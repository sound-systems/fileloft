use http::HeaderMap;

use crate::config::Config;
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

/// Build a header value from a `u64` (decimal digits are valid HTTP header bytes).
pub fn u64_header(n: u64) -> http::HeaderValue {
    let s = n.to_string();
    http::HeaderValue::try_from(s.as_str()).unwrap_or_else(|_| http::HeaderValue::from_static("0"))
}

/// Absolute origin for `Location` (scheme + host, no path) when `base_url` is unset.
pub(crate) fn request_base_url(config: &Config, headers: &HeaderMap) -> String {
    if let Some(ref base) = config.base_url {
        return base.trim_end_matches('/').to_string();
    }
    let scheme = if config.trust_forwarded_headers {
        headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next().map(str::trim))
            .filter(|s| !s.is_empty())
            .unwrap_or("http")
    } else {
        "http"
    };
    let host = if config.trust_forwarded_headers {
        headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("localhost")
    } else {
        headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost")
    };
    format!("{scheme}://{host}")
}

/// Build a header value from a static string.
pub fn static_header(s: &'static str) -> http::HeaderValue {
    http::HeaderValue::from_static(s)
}
