use http::{HeaderMap, StatusCode};

use crate::{
    error::TusError,
    handler::{TusRequest, TusResponse},
    lock::SendLocker,
    proto::{
        HDR_UPLOAD_DEFER_LENGTH, HDR_UPLOAD_LENGTH, HDR_UPLOAD_METADATA, HDR_UPLOAD_OFFSET,
    },
    store::{SendDataStore, SendUpload},
    util::{static_header, u64_header},
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

    let id = req
        .upload_id
        .as_deref()
        .ok_or(TusError::InvalidUploadId)?
        .into();

    let upload = h.store.get_upload(&id).await?;
    let info = upload.get_info().await?;

    // 410 Gone if expired
    if let Some(expires_at) = info.expires_at {
        if chrono::Utc::now() > expires_at {
            return Err(TusError::Gone);
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert(HDR_UPLOAD_OFFSET, u64_header(info.offset));

    // Upload-Length is omitted when size is deferred and not yet declared
    if let Some(size) = info.size {
        headers.insert(HDR_UPLOAD_LENGTH, u64_header(size));
    } else if info.size_is_deferred {
        headers.insert(HDR_UPLOAD_DEFER_LENGTH, static_header("1"));
    }

    if !info.metadata.is_empty() {
        let encoded = info.metadata.encode();
        if let Ok(v) = encoded.parse() {
            headers.insert(HDR_UPLOAD_METADATA, v);
        }
    }

    if let Some(expires_at) = info.expires_at {
        let formatted = expires_at
            .format(crate::proto::HTTP_DATE_FORMAT)
            .to_string();
        if let Ok(v) = formatted.parse::<http::HeaderValue>() {
            headers.insert(crate::proto::HDR_UPLOAD_EXPIRES, v);
        }
    }

    Ok(h.response(StatusCode::NO_CONTENT, headers, bytes::Bytes::new()))
}
