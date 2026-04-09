use http::{HeaderMap, HeaderValue, StatusCode};

use crate::{
    error::TusError,
    handler::{TusBody, TusRequest, TusResponse},
    lock::SendLocker,
    proto::{HDR_CONTENT_LENGTH, HDR_CONTENT_TYPE},
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
    if !h.config.enable_download {
        return Err(TusError::MethodNotAllowed);
    }

    let id = req
        .upload_id
        .as_deref()
        .ok_or(TusError::InvalidUploadId)?
        .into();

    let _lock = if let Some(locker) = &h.locker {
        Some(
            tokio::time::timeout(h.config.lock_timeout, locker.acquire(&id))
                .await
                .map_err(|_| TusError::LockTimeout(id.to_string()))??,
        )
    } else {
        None
    };

    let upload = h.store.get_upload(&id).await?;
    let info = upload.get_info().await?;

    if !info.is_complete() {
        return Err(TusError::UploadNotReadyForDownload);
    }

    let reader = upload.read_content().await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        HDR_CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Some(len) = info.size {
        headers.insert(HDR_CONTENT_LENGTH, crate::util::u64_header(len));
    }

    Ok(h.response_with_body(StatusCode::OK, headers, TusBody::Reader(reader)))
}
