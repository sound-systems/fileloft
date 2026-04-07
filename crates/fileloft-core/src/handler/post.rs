use http::{HeaderMap, StatusCode};

use crate::{
    error::TusError,
    handler::{TusRequest, TusResponse},
    hooks::HookEvent,
    info::{UploadId, UploadInfo},
    lock::SendLocker,
    proto::{
        HDR_CONTENT_TYPE, HDR_LOCATION, HDR_UPLOAD_CONCAT, HDR_UPLOAD_EXPIRES,
        HDR_UPLOAD_LENGTH, HDR_UPLOAD_METADATA, HDR_UPLOAD_OFFSET,
    },
    store::{SendDataStore, SendUpload as _},
    util::{has_defer_length, parse_upload_length, request_base_url, u64_header},
};

use super::TusHandler;

pub(super) async fn handle<S, L>(
    h: &TusHandler<S, L>,
    mut req: TusRequest,
) -> Result<TusResponse, TusError>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    crate::util::check_tus_resumable(&req.headers)?;

    if !h.config.extensions.creation {
        return Err(TusError::ExtensionNotEnabled("creation"));
    }

    // --- Parse Upload-Concat (concatenation extension) ---
    let concat_header = req
        .headers
        .get(HDR_UPLOAD_CONCAT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let is_final = concat_header
        .as_deref()
        .map(|v| v.starts_with("final;"))
        .unwrap_or(false);
    let is_partial = concat_header.as_deref() == Some("partial");

    if is_final && !h.config.extensions.concatenation {
        return Err(TusError::ExtensionNotEnabled("concatenation"));
    }

    // --- Parse size ---
    let upload_length = parse_upload_length(&req.headers)?;
    let defer = has_defer_length(&req.headers);

    if upload_length.is_none() && !defer && !is_final {
        return Err(TusError::MissingUploadLength);
    }
    if defer && !h.config.extensions.creation_defer_length {
        return Err(TusError::ExtensionNotEnabled("creation-defer-length"));
    }

    // Max-size guard
    if let Some(size) = upload_length {
        if h.config.max_size > 0 && size > h.config.max_size {
            return Err(TusError::EntityTooLarge { max: h.config.max_size });
        }
    }

    // --- Parse metadata ---
    let metadata = match req.headers.get(HDR_UPLOAD_METADATA) {
        None => crate::info::Metadata::default(),
        Some(v) => crate::info::Metadata::parse(v.to_str().unwrap_or(""))?,
    };

    // --- Build UploadInfo ---
    let id = UploadId::new();
    let mut info = UploadInfo::new(id.clone(), upload_length);
    info.metadata = metadata;
    info.size_is_deferred = defer;
    info.is_partial = is_partial;
    info.is_final = is_final;

    // Collect partial upload IDs for final concatenation
    if is_final {
        let urls = parse_concat_final_urls(concat_header.as_deref().unwrap_or(""))?;
        info.partial_uploads = urls
            .into_iter()
            .map(|url| extract_upload_id_from_url(&url, &h.config.base_path))
            .collect::<Result<Vec<_>, _>>()?;
    }

    // Expiration
    if h.config.extensions.expiration {
        if let Some(ttl) = h.config.extensions.expiration_ttl {
            info.expires_at = Some(
                chrono::Utc::now()
                    + chrono::Duration::from_std(ttl)
                        .map_err(|e| TusError::Internal(e.to_string()))?,
            );
        }
    }

    // --- pre_create hook ---
    if let Some(cb) = &h.config.hooks.pre_create {
        let changes = cb(info.clone()).await?;
        if let Some(new_id) = changes.id {
            info.id = new_id;
        }
        if let Some(new_meta) = changes.metadata {
            info.metadata = new_meta;
        }
        if let Some(new_storage) = changes.storage {
            info.storage = new_storage;
        }
    }

    // --- Create upload slot ---
    let mut upload = h.store.create_upload(info.clone()).await?;

    // --- Handle final concatenation ---
    if is_final && h.config.extensions.concatenation {
        let partials = fetch_partial_infos(h, &info.partial_uploads).await?;
        upload.concatenate(&partials).await?;
    }

    // --- Handle creation-with-upload body ---
    let mut bytes_written: u64 = 0;
    if h.config.extensions.creation_with_upload {
        if let Some(ct) = req.headers.get(HDR_CONTENT_TYPE) {
            if ct
                .to_str()
                .unwrap_or("")
                .starts_with(crate::proto::CONTENT_TYPE_OCTET_STREAM)
            {
                if let Some(ref mut body) = req.body {
                    bytes_written = upload.write_chunk(0, body.as_mut()).await?;
                }
            }
        }
    }

    let mut final_info = upload.get_info().await?;

    if let Some(declared) = final_info.size {
        if bytes_written > declared {
            return Err(TusError::ExceedsUploadLength {
                declared,
                end: bytes_written,
            });
        }
    }

    // Optional: remove partial uploads after a successful final concatenation.
    if is_final
        && h.config.extensions.concatenation
        && h.config.extensions.cleanup_concat_partials
    {
        for partial_id in &final_info.partial_uploads {
            let u = h.store.get_upload(partial_id).await?;
            u.delete().await?;
        }
        final_info = upload.get_info().await?;
    }

    // Fire UploadCreated event
    h.emit(HookEvent::UploadCreated {
        info: final_info.clone(),
    });

    // Check if already complete (e.g. Upload-Length: 0 or creation-with-upload finished it)
    if final_info.is_complete() {
        if let Some(cb) = &h.config.hooks.pre_finish {
            cb(final_info.clone()).await?;
        }
        upload.finalize().await?;
        h.emit(HookEvent::UploadFinished {
            info: final_info.clone(),
        });
    }

    // --- Build 201 response ---
    let location = build_location(h, &final_info.id, &req);
    let mut headers = HeaderMap::new();
    headers.insert(
        HDR_LOCATION,
        location
            .parse()
            .map_err(|_| TusError::Internal("bad location".into()))?,
    );
    headers.insert(HDR_UPLOAD_OFFSET, u64_header(final_info.offset));

    if let Some(size) = final_info.size {
        headers.insert(HDR_UPLOAD_LENGTH, u64_header(size));
    }
    if h.config.extensions.expiration {
        if let Some(expires_at) = final_info.expires_at {
            let formatted = expires_at
                .format(crate::proto::HTTP_DATE_FORMAT)
                .to_string();
            if let Ok(v) = formatted.parse() {
                headers.insert(HDR_UPLOAD_EXPIRES, v);
            }
        }
    }

    Ok(h.response(StatusCode::CREATED, headers, bytes::Bytes::new()))
}

fn build_location<S, L>(h: &TusHandler<S, L>, id: &UploadId, req: &TusRequest) -> String
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let base = request_base_url(&h.config, &req.headers);
    let path = h.config.base_path.trim_end_matches('/');
    format!("{base}{path}/{id}")
}

fn parse_concat_final_urls(header: &str) -> Result<Vec<String>, TusError> {
    let rest = header.strip_prefix("final;").unwrap_or("");
    let urls: Vec<String> = rest.split_whitespace().map(str::to_owned).collect();
    if urls.is_empty() {
        return Err(TusError::EmptyConcatenation);
    }
    Ok(urls)
}

fn extract_upload_id_from_url(url: &str, base_path: &str) -> Result<UploadId, TusError> {
    let path = url.trim();
    let base = base_path.trim_end_matches('/');
    if let Some(pos) = path.rfind(base) {
        let after = &path[pos + base.len()..];
        let id = after.trim_start_matches('/');
        if !id.is_empty() {
            return Ok(UploadId::from(id));
        }
    }
    // Fallback: last path segment
    let id = path.rsplit('/').next().unwrap_or("").trim();
    if id.is_empty() {
        return Err(TusError::InvalidUploadId);
    }
    Ok(UploadId::from(id))
}

async fn fetch_partial_infos<S, L>(
    h: &TusHandler<S, L>,
    ids: &[UploadId],
) -> Result<Vec<UploadInfo>, TusError>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let mut infos = Vec::with_capacity(ids.len());
    for id in ids {
        let upload = h.store.get_upload(id).await?;
        let info = upload.get_info().await?;
        if !info.is_complete() {
            return Err(TusError::PartialUploadIncomplete(id.to_string()));
        }
        infos.push(info);
    }
    Ok(infos)
}
