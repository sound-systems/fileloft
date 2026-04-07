//! Amazon S3 (and S3-compatible) backend for fileloft.
//!
//! Layout (mirrors tusd / GCS conventions):
//!
//! - Metadata: `{prefix}{id}.info` — JSON-serialised [`UploadInfo`].
//! - Parts:    `{prefix}{id}_part_{n}` — one object per PATCH chunk.
//! - Final:    `{prefix}{id}` — assembled from parts when the upload completes.

use std::mem;

use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, instrument};

use crate::error::S3StoreError;
use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};

/// Minimum size (except the last part) for S3 multipart uploads (5 MiB).
const MIN_MULTIPART_PART: usize = 5 * 1024 * 1024;

/// S3 allows at most 10_000 parts per multipart upload.
const MAX_MULTIPART_PARTS: i32 = 10_000;

/// A single-object streaming backend for tus uploads.
///
/// The AWS SDK client uses an internal `Arc`, so cloning is cheap.
///
/// # Authentication
///
/// Uses the default AWS credential chain unless you pass a pre-built
/// [`Client`] via [`S3StoreBuilder::with_client`].
pub struct S3Store {
    client: Client,
    bucket: String,
    /// Object-name prefix (may be empty). Always ends with `/` if non-empty.
    prefix: String,
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Store")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

// --- Construction -----------------------------------------------------------

/// Builder for [`S3Store`].
///
/// ```no_run
/// # async fn demo() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// use fileloft_store_s3::S3Store;
///
/// let store = S3Store::builder("my-bucket")
///     .prefix("uploads/")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct S3StoreBuilder {
    bucket: String,
    prefix: String,
    endpoint_url: Option<String>,
    force_path_style: bool,
    region: Option<String>,
    client: Option<Client>,
}

impl S3StoreBuilder {
    /// Set an object-name prefix (e.g. `"uploads/"`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Custom base endpoint for S3-compatible services (MinIO, R2, etc.).
    pub fn endpoint_url(mut self, url: impl Into<String>) -> Self {
        self.endpoint_url = Some(url.into());
        self
    }

    /// Use path-style addressing (`bucket.host/key`) — often required for MinIO.
    pub fn force_path_style(mut self, yes: bool) -> Self {
        self.force_path_style = yes;
        self
    }

    /// Override the signing / client region (optional).
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Provide a pre-configured S3 client (skips loading shared config).
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Build the store, loading AWS config when no client was supplied.
    pub async fn build(self) -> Result<S3Store, S3StoreError> {
        if self.bucket.is_empty() {
            return Err(S3StoreError::BucketEmpty);
        }

        let client = match self.client {
            Some(c) => c,
            None => {
                let conf = aws_config::defaults(BehaviorVersion::latest())
                    .load()
                    .await;

                let mut b = aws_sdk_s3::config::Builder::from(&conf);
                if let Some(url) = &self.endpoint_url {
                    b = b.endpoint_url(url);
                }
                if self.force_path_style {
                    b = b.force_path_style(true);
                }
                if let Some(r) = &self.region {
                    b = b.region(Region::new(r.clone()));
                }
                Client::from_conf(b.build())
            }
        };

        let mut prefix = self.prefix;
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        Ok(S3Store {
            client,
            bucket: self.bucket,
            prefix,
        })
    }
}

impl S3Store {
    /// Start building a new [`S3Store`] for the given bucket name.
    pub fn builder(bucket: impl Into<String>) -> S3StoreBuilder {
        S3StoreBuilder {
            bucket: bucket.into(),
            prefix: String::new(),
            endpoint_url: None,
            force_path_style: false,
            region: None,
            client: None,
        }
    }
}

// --- Low-level S3 operations ------------------------------------------------

impl S3Store {
    fn info_key(&self, id: &str) -> String {
        format!("{}{}.info", self.prefix, id)
    }

    async fn write_info_object(&self, info: &UploadInfo) -> Result<(), TusError> {
        let key = self.info_key(info.id.as_str());
        let id = info.id.as_str();
        let json = serde_json::to_vec_pretty(info)
            .map_err(|e| TusError::Internal(format!("serialize info: {e}")))?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(Bytes::from(json)))
            .send()
            .await
            .map_err(|e| s3_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_object(&self, id: &str) -> Result<UploadInfo, TusError> {
        let key = self.info_key(id);

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| s3_err(e, id, "read info"))?;

        let mut body = resp.body.into_async_read();
        let mut buf = Vec::new();
        body
            .read_to_end(&mut buf)
            .await
            .map_err(TusError::Io)?;

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }
}

// --- DataStore impl ---------------------------------------------------------

impl SendDataStore for S3Store {
    type UploadType = S3Upload;

    #[instrument(skip(self, info), fields(upload_id = %info.id))]
    async fn create_upload(&self, info: UploadInfo) -> Result<S3Upload, TusError> {
        self.write_info_object(&info).await?;
        debug!("created upload");

        Ok(S3Upload {
            id: info.id.clone(),
            client: self.client.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
        })
    }

    #[instrument(skip(self))]
    async fn get_upload(&self, id: &UploadId) -> Result<S3Upload, TusError> {
        let _info = self.read_info_object(id.as_str()).await?;

        Ok(S3Upload {
            id: id.clone(),
            client: self.client.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
        })
    }
}

// --- Upload handle ----------------------------------------------------------

/// Handle to a single S3-backed upload.
pub struct S3Upload {
    id: UploadId,
    client: Client,
    bucket: String,
    prefix: String,
}

impl S3Upload {
    fn data_key(&self) -> String {
        format!("{}{}", self.prefix, self.id.as_str())
    }

    fn info_key(&self) -> String {
        format!("{}{}.info", self.prefix, self.id.as_str())
    }

    fn part_key(&self, index: u32) -> String {
        format!("{}{}_part_{}", self.prefix, self.id.as_str(), index)
    }

    async fn write_info_object(&self, info: &UploadInfo) -> Result<(), TusError> {
        let key = self.info_key();
        let id = self.id.as_str();
        let json = serde_json::to_vec_pretty(info)
            .map_err(|e| TusError::Internal(format!("serialize info: {e}")))?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(Bytes::from(json)))
            .send()
            .await
            .map_err(|e| s3_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_object(&self) -> Result<UploadInfo, TusError> {
        let key = self.info_key();
        let id = self.id.as_str();

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| s3_err(e, id, "read info"))?;

        let mut body = resp.body.into_async_read();
        let mut buf = Vec::new();
        body
            .read_to_end(&mut buf)
            .await
            .map_err(TusError::Io)?;

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }

    async fn list_parts(&self) -> Result<Vec<String>, TusError> {
        let id = self.id.as_str();
        let part_prefix = format!("{}{}_part_", self.prefix, id);
        let mut names = Vec::new();
        let mut token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&part_prefix);
            if let Some(ref t) = token {
                req = req.continuation_token(t);
            }
            let page = req.send().await.map_err(|e| s3_err(e, id, "list parts"))?;

            for obj in page.contents() {
                if let Some(k) = obj.key() {
                    if !k.is_empty() {
                        names.push(k.to_string());
                    }
                }
            }

            let truncated = page.is_truncated().unwrap_or(false);
            if !truncated {
                break;
            }
            token = page.next_continuation_token().map(|s| s.to_string());
            if token.as_ref().map_or(true, |t| t.is_empty()) {
                break;
            }
        }

        names.sort_by(|a, b| part_index(a).cmp(&part_index(b)));
        Ok(names)
    }

    async fn delete_object_key(&self, key: &str) {
        let _ = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;
    }

    /// Stream sources in order into a multipart upload at `dest_key`, then delete `delete_after_ok`.
    async fn multipart_assemble_from_sources(
        &self,
        dest_key: &str,
        source_keys: &[String],
        delete_after_ok: &[String],
    ) -> Result<(), TusError> {
        let log_id = self.id.as_str();

        if source_keys.is_empty() {
            return Ok(());
        }

        let create = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(dest_key)
            .send()
            .await
            .map_err(|e| s3_err(e, log_id, "create multipart"))?;

        let upload_id = match create.upload_id() {
            Some(u) => u.to_string(),
            None => {
                return Err(TusError::Internal(
                    "S3 create multipart: missing upload_id".into(),
                ));
            }
        };

        let mut completed: Vec<CompletedPart> = Vec::new();
        let mut part_number: i32 = 1;
        let mut buffer: Vec<u8> = Vec::new();

        let abort_res = async {
            let run = self.multipart_upload_parts(
                dest_key,
                &upload_id,
                source_keys,
                &mut buffer,
                &mut completed,
                &mut part_number,
                log_id,
            )
            .await;

            if let Err(e) = run {
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(dest_key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                return Err(e);
            }

            if completed.is_empty() {
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(dest_key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                return Err(TusError::Internal(
                    "S3 multipart: no parts produced (unexpected)".into(),
                ));
            }

            let completed_mpu = CompletedMultipartUpload::builder()
                .set_parts(Some(completed))
                .build();

            self.client
                .complete_multipart_upload()
                .bucket(&self.bucket)
                .key(dest_key)
                .upload_id(&upload_id)
                .multipart_upload(completed_mpu)
                .send()
                .await
                .map_err(|e| s3_err(e, log_id, "complete multipart"))?;

            for key in delete_after_ok {
                self.delete_object_key(key).await;
            }

            Ok(())
        };

        abort_res.await
    }

    async fn multipart_upload_parts(
        &self,
        dest_key: &str,
        upload_id: &str,
        source_keys: &[String],
        buffer: &mut Vec<u8>,
        completed: &mut Vec<CompletedPart>,
        part_number: &mut i32,
        log_id: &str,
    ) -> Result<(), TusError> {
        let n_sources = source_keys.len();

        for (src_idx, key) in source_keys.iter().enumerate() {
            let resp = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| s3_err(e, log_id, "get source for assembly"))?;

            let mut reader = resp.body.into_async_read();
            let mut read_buf = vec![0u8; 64 * 1024];

            loop {
                let n = reader
                    .read(&mut read_buf)
                    .await
                    .map_err(TusError::Io)?;
                let eof_on_object = n == 0;

                if n > 0 {
                    buffer.extend_from_slice(&read_buf[..n]);
                }

                let more_keys_after = src_idx + 1 < n_sources;
                let more_data_coming = !eof_on_object || more_keys_after;

                while buffer.len() >= MIN_MULTIPART_PART {
                    if buffer.len() == MIN_MULTIPART_PART && !more_data_coming {
                        break;
                    }

                    let tail = buffer.split_off(MIN_MULTIPART_PART);
                    let part_bytes = mem::replace(buffer, tail);
                    self.upload_one_part(
                        dest_key,
                        upload_id,
                        part_bytes,
                        part_number,
                        completed,
                        log_id,
                    )
                    .await?;
                }

                if eof_on_object {
                    break;
                }
            }
        }

        if buffer.is_empty() && completed.is_empty() {
            // All source objects are empty; S3 requires at least one part.
            self.upload_one_part(
                dest_key,
                upload_id,
                Vec::new(),
                part_number,
                completed,
                log_id,
            )
            .await?;
        } else if !buffer.is_empty() {
            let rest = mem::take(buffer);
            self.upload_one_part(
                dest_key,
                upload_id,
                rest,
                part_number,
                completed,
                log_id,
            )
            .await?;
        }

        Ok(())
    }

    async fn upload_one_part(
        &self,
        dest_key: &str,
        upload_id: &str,
        part_bytes: Vec<u8>,
        part_number: &mut i32,
        completed: &mut Vec<CompletedPart>,
        log_id: &str,
    ) -> Result<(), TusError> {
        if *part_number > MAX_MULTIPART_PARTS {
            return Err(TusError::Internal(
                "S3 multipart: exceeded maximum part count (10000)".into(),
            ));
        }

        let body = ByteStream::from(Bytes::from(part_bytes));

        let up = self
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(dest_key)
            .upload_id(upload_id)
            .part_number(*part_number)
            .body(body)
            .send()
            .await
            .map_err(|e| s3_err(e, log_id, "upload part"))?;

        let etag = match up.e_tag() {
            Some(t) => t.to_string(),
            None => {
                return Err(TusError::Internal(
                    "S3 upload part: missing ETag".into(),
                ));
            }
        };

        let cp = CompletedPart::builder()
            .e_tag(&etag)
            .part_number(*part_number)
            .build();

        completed.push(cp);
        *part_number += 1;
        Ok(())
    }
}

impl SendUpload for S3Upload {
    #[instrument(skip(self, reader), fields(upload_id = %self.id))]
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        let mut info = self.read_info_object().await?;
        if info.offset != offset {
            return Err(TusError::OffsetMismatch {
                expected: info.offset,
                actual: offset,
            });
        }

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        let n = buf.len() as u64;

        let end_offset = offset.checked_add(n).ok_or_else(|| {
            TusError::Internal("upload offset overflow".into())
        })?;
        if let Some(declared) = info.size {
            if end_offset > declared {
                return Err(TusError::ExceedsUploadLength {
                    declared,
                    end: end_offset,
                });
            }
        }

        let part_index = self.list_parts().await?.len() as u32;
        let part_key = self.part_key(part_index);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&part_key)
            .body(ByteStream::from(Bytes::from(buf)))
            .send()
            .await
            .map_err(|e| s3_err(e, self.id.as_str(), "write part"))?;

        info.offset = end_offset;
        self.write_info_object(&info).await?;

        debug!(bytes = n, new_offset = end_offset, "wrote chunk");
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        self.read_info_object().await
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn finalize(&mut self) -> Result<(), TusError> {
        let parts = self.list_parts().await?;
        if parts.is_empty() {
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(&self.data_key())
                .body(ByteStream::from(Bytes::new()))
                .send()
                .await
                .map_err(|e| s3_err(e, self.id.as_str(), "write empty final"))?;
            return Ok(());
        }

        let dest_key = self.data_key();
        let to_delete: Vec<String> = parts.clone();
        self.multipart_assemble_from_sources(&dest_key, &parts, &to_delete)
            .await?;

        debug!("finalized upload");
        Ok(())
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn delete(self) -> Result<(), TusError> {
        self.delete_object_key(&self.data_key()).await;
        self.delete_object_key(&self.info_key()).await;

        let parts = match self.list_parts().await {
            Ok(p) => p,
            Err(_) => Vec::new(),
        };
        for part_key in parts {
            self.delete_object_key(&part_key).await;
        }

        debug!("terminated upload");
        Ok(())
    }

    async fn declare_length(&mut self, length: u64) -> Result<(), TusError> {
        let mut info = self.read_info_object().await?;
        if info.size.is_some() {
            return Err(TusError::UploadLengthAlreadySet);
        }
        info.size = Some(length);
        info.size_is_deferred = false;
        self.write_info_object(&info).await
    }

    #[instrument(skip(self, partials), fields(upload_id = %self.id, n_partials = partials.len()))]
    async fn concatenate(&mut self, partials: &[UploadInfo]) -> Result<(), TusError> {
        let source_keys: Vec<String> = partials
            .iter()
            .map(|p| format!("{}{}", self.prefix, p.id.as_str()))
            .collect();

        let dest_key = self.data_key();
        let to_delete = source_keys.clone();

        self.multipart_assemble_from_sources(&dest_key, &source_keys, &to_delete)
            .await?;

        let total: u64 = partials.iter().filter_map(|p| p.size).sum();
        let mut info = self.read_info_object().await?;
        info.size = Some(total);
        info.offset = total;
        info.is_final = true;
        self.write_info_object(&info).await?;

        debug!("concatenated {} partials", partials.len());
        Ok(())
    }
}

// --- Helpers ----------------------------------------------------------------

/// Extract the numeric part index from a key like `…_part_7`.
fn part_index(key: &str) -> u32 {
    key.rsplit('_')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

fn is_not_found<E: ProvideErrorMetadata>(err: &SdkError<E>) -> bool {
    match err {
        SdkError::ServiceError(se) => {
            let code = se.err().meta().code();
            matches!(
                code,
                Some("NoSuchKey") | Some("NotFound") | Some("404")
            )
        }
        SdkError::ResponseError(re) => re.raw().status().as_u16() == 404,
        _ => false,
    }
}

fn s3_err<E>(e: SdkError<E>, upload_id: &str, op: &str) -> TusError
where
    E: std::error::Error + ProvideErrorMetadata + Send + Sync + 'static,
{
    if is_not_found(&e) {
        TusError::NotFound(upload_id.to_string())
    } else {
        error!(%e, op, "S3 operation failed");
        TusError::Internal(format!("S3 {op}: {e}"))
    }
}
