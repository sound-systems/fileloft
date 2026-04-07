use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::{
    checksum::{parse_checksum_header, ChecksumReader},
    error::TusError,
    handler::{TusRequest, TusResponse},
    hooks::HookEvent,
    lock::Locker,
    proto::{CONTENT_TYPE_OCTET_STREAM, HDR_CONTENT_TYPE, HDR_UPLOAD_EXPIRES, HDR_UPLOAD_OFFSET},
    store::{DataStore, Upload as _},
    util::{check_tus_resumable, parse_upload_length, parse_upload_offset, u64_header},
};

use super::TusHandler;

pub(super) async fn handle<S, L>(
    h: &TusHandler<S, L>,
    req: TusRequest,
) -> Result<TusResponse, TusError>
where
    S: DataStore + Send + Sync + 'static,
    L: Locker + Send + Sync + 'static,
{
    check_tus_resumable(&req.headers)?;

    // Validate Content-Type
    let ct = req
        .headers
        .get(HDR_CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ct.starts_with(CONTENT_TYPE_OCTET_STREAM) {
        return Err(TusError::WrongContentType(ct.to_string()));
    }

    let client_offset = parse_upload_offset(&req.headers)?;

    let id = req
        .upload_id
        .as_deref()
        .ok_or(TusError::InvalidUploadId)?
        .into();

    // Acquire lock
    let _lock = if let Some(locker) = &h.locker {
        Some(
            tokio::time::timeout(h.config.lock_timeout, locker.acquire(&id))
                .await
                .map_err(|_| TusError::LockTimeout(id.to_string()))??,
        )
    } else {
        None
    };

    let mut upload = h.store.get_upload(&id).await?;
    let info = upload.get_info().await?;

    // Reject PATCH on final concatenated uploads
    if info.is_final {
        return Err(TusError::PatchOnFinalUpload);
    }

    // Check offset matches
    if info.offset != client_offset {
        return Err(TusError::OffsetMismatch {
            expected: info.offset,
            actual: client_offset,
        });
    }

    // Check expiry
    if let Some(expires_at) = info.expires_at {
        if chrono::Utc::now() > expires_at {
            return Err(TusError::Gone);
        }
    }

    // Handle Upload-Length on deferred-length upload
    if info.size_is_deferred && info.size.is_none() {
        if let Some(declared_size) = parse_upload_length(&req.headers)? {
            if h.config.max_size > 0 && declared_size > h.config.max_size {
                return Err(TusError::EntityTooLarge { max: h.config.max_size });
            }
            upload.declare_length(declared_size).await?;
        }
    }

    // Parse optional checksum header
    let checksum = req
        .headers
        .get(crate::proto::HDR_UPLOAD_CHECKSUM)
        .and_then(|v| v.to_str().ok())
        .map(parse_checksum_header)
        .transpose()?;

    let body = req
        .body
        .ok_or_else(|| TusError::Internal("PATCH missing body".into()))?;

    // Write chunk — with or without checksum wrapping
    let _bytes_written = if let Some((algorithm, expected_hash)) = checksum {
        let mut checksum_reader = ChecksumReader::new(body, algorithm, expected_hash);
        let n = upload.write_chunk(client_offset, &mut checksum_reader).await?;
        checksum_reader.verify()?;
        n
    } else {
        let mut body = body;
        upload.write_chunk(client_offset, body.as_mut()).await?
    };

    let new_info = upload.get_info().await?;

    // Emit progress event
    h.emit(HookEvent::UploadProgress {
        info: new_info.clone(),
    });

    // Finalize if complete
    if new_info.is_complete() {
        if let Some(cb) = &h.config.hooks.pre_finish {
            cb(new_info.clone()).await?;
        }
        upload.finalize().await?;
        h.emit(HookEvent::UploadFinished {
            info: new_info.clone(),
        });
    }

    // Build 204 response
    let mut headers = HeaderMap::new();
    headers.insert(HDR_UPLOAD_OFFSET, u64_header(new_info.offset));

    if h.config.extensions.expiration {
        if let Some(expires_at) = new_info.expires_at {
            let formatted = expires_at
                .format(crate::proto::HTTP_DATE_FORMAT)
                .to_string();
            if let Ok(v) = formatted.parse() {
                headers.insert(HDR_UPLOAD_EXPIRES, v);
            }
        }
    }

    Ok(h.response(StatusCode::NO_CONTENT, headers, Bytes::new()))
}
