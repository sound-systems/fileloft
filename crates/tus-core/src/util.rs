use http::HeaderMap;

use crate::error::TusError;
use crate::proto::{HDR_TUS_RESUMABLE, TUS_VERSION};

/// Verify the `Tus-Resumable` header is present and matches our version.
pub fn check_tus_resumable(headers: &HeaderMap) -> Result<(), TusError> {
    match headers.get(HDR_TUS_RESUMABLE) {
        None => Err(TusError::MissingTusResumable),
        Some(v) => {
            let s = v.to_str().unwrap_or("");
            if s == TUS_VERSION {
                Ok(())
            } else {
                Err(TusError::UnsupportedVersion {
                    version: s.to_string(),
                })
            }
        }
    }
}

/// Parse `Upload-Offset` from headers. Returns `Err` if missing or not a valid u64.
pub fn parse_upload_offset(headers: &HeaderMap) -> Result<u64, TusError> {
    headers
        .get(crate::proto::HDR_UPLOAD_OFFSET)
        .ok_or(TusError::MissingUploadOffset)?
        .to_str()
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(TusError::MissingUploadOffset)
}

/// Parse `Upload-Length` from headers. Returns `None` if absent.
pub fn parse_upload_length(headers: &HeaderMap) -> Result<Option<u64>, TusError> {
    match headers.get(crate::proto::HDR_UPLOAD_LENGTH) {
        None => Ok(None),
        Some(v) => v
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Some)
            .ok_or_else(|| TusError::InvalidMetadata("invalid Upload-Length value".into())),
    }
}

/// Returns `true` if the request has `Upload-Defer-Length: 1`.
pub fn has_defer_length(headers: &HeaderMap) -> bool {
    headers
        .get(crate::proto::HDR_UPLOAD_DEFER_LENGTH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

/// Build a header value from a `u64`.
pub fn u64_header(n: u64) -> http::HeaderValue {
    http::HeaderValue::from_str(&n.to_string()).expect("u64 is always a valid header value")
}

/// Build a header value from a static string.
pub fn static_header(s: &'static str) -> http::HeaderValue {
    http::HeaderValue::from_static(s)
}
