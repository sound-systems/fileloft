use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::{
    error::TusError,
    handler::{TusRequest, TusResponse},
    hooks::HookEvent,
    lock::SendLocker,
    store::{SendDataStore, SendUpload as _},
};

use super::TusHandler;

pub(super) async fn handle<S, L>(
    h: &TusHandler<S, L>,
    req: &TusRequest,
) -> Result<TusResponse, TusError>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    crate::util::check_tus_resumable(&req.headers)?;

    if !h.config.extensions.termination {
        return Err(TusError::ExtensionNotEnabled("termination"));
    }

    let id = req
        .upload_id
        .as_deref()
        .ok_or(TusError::InvalidUploadId)?
        .into();

    // pre_terminate hook
    if let Some(cb) = &h.config.hooks.pre_terminate {
        let upload = h.store.get_upload(&id).await?;
        let info = upload.get_info().await?;
        cb(info).await?;
    }

    // Acquire lock before deletion
    let _lock = if let Some(locker) = &h.locker {
        Some(
            tokio::time::timeout(h.config.lock_timeout, locker.acquire(&id))
                .await
                .map_err(|_| TusError::LockTimeout(id.to_string()))??,
        )
    } else {
        None
    };

    let upload_id = id.clone();
    let upload = h.store.get_upload(&id).await?;
    upload.delete().await?;

    h.emit(HookEvent::UploadTerminated { id: upload_id });

    Ok(h.response(StatusCode::NO_CONTENT, HeaderMap::new(), Bytes::new()))
}
