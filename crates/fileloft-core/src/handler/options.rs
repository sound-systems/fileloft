use http::{HeaderMap, StatusCode};

use crate::{
    error::TusError,
    handler::{TusRequest, TusResponse},
    lock::SendLocker,
    proto::*,
    store::SendDataStore,
    util::static_header,
};

use super::TusHandler;

pub(super) async fn handle<S, L>(
    h: &TusHandler<S, L>,
    _req: &TusRequest,
) -> Result<TusResponse, TusError>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let mut headers = HeaderMap::new();

    // Advertise supported versions
    headers.insert(HDR_TUS_VERSION, static_header(TUS_VERSION));

    // Build the extension list from the current Config
    let ext = &h.config.extensions;
    let mut exts = Vec::new();
    if ext.creation {
        exts.push(EXT_CREATION);
    }
    if ext.creation_with_upload {
        exts.push(EXT_CREATION_WITH_UPLOAD);
    }
    if ext.creation_defer_length {
        exts.push(EXT_CREATION_DEFER_LENGTH);
    }
    if ext.expiration {
        exts.push(EXT_EXPIRATION);
    }
    if ext.checksum {
        exts.push(EXT_CHECKSUM);
    }
    if ext.checksum_trailer && CHECKSUM_TRAILER_IMPLEMENTED {
        exts.push(EXT_CHECKSUM_TRAILER);
    }
    if ext.termination {
        exts.push(EXT_TERMINATION);
    }
    if ext.concatenation {
        exts.push(EXT_CONCATENATION);
    }

    if !exts.is_empty() {
        let ext_str = exts.join(",");
        headers.insert(
            HDR_TUS_EXTENSION,
            ext_str.parse().map_err(|_| TusError::Internal("bad extension header".into()))?,
        );
    }

    if ext.checksum {
        headers.insert(
            HDR_TUS_CHECKSUM_ALGORITHM,
            crate::checksum::algorithms_header()
                .parse()
                .map_err(|_| TusError::Internal("bad algorithm header".into()))?,
        );
    }

    if h.config.max_size > 0 {
        headers.insert(HDR_TUS_MAX_SIZE, crate::util::u64_header(h.config.max_size));
    }

    if h.config.enable_cors {
        headers.insert(
            HDR_ACCESS_CONTROL_ALLOW_METHODS,
            static_header("OPTIONS,HEAD,POST,PATCH,DELETE"),
        );
        headers.insert(
            HDR_ACCESS_CONTROL_ALLOW_HEADERS,
            static_header(
                "Tus-Resumable,Upload-Length,Upload-Offset,Upload-Metadata,\
                 Upload-Defer-Length,Upload-Checksum,Upload-Concat,Content-Type",
            ),
        );
        headers.insert(HDR_ACCESS_CONTROL_MAX_AGE, static_header("86400"));
    }

    Ok(h.response(StatusCode::NO_CONTENT, headers, bytes::Bytes::new()))
}
