//! Google Cloud Storage backend for fileloft.
//!
//! Layout (mirrors tusd conventions):
//!
//! - Metadata: `{prefix}{id}.info` — JSON-serialised [`UploadInfo`].
//! - Parts:    `{prefix}{id}_part_{n}` — one object per PATCH chunk.
//! - Final:    `{prefix}{id}` — composed from parts when the upload completes.

use bytes::Bytes;
use google_cloud_storage::client::{Storage, StorageControl};
use google_cloud_storage::model::compose_object_request::SourceObject;
use google_cloud_storage::model::Object;
use std::io::Cursor;

use tokio::io::AsyncReadExt;
use tracing::{debug, error, instrument};

use crate::error::GcsStoreError;
use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};

/// Maximum number of source objects in a single GCS compose request.
const MAX_COMPOSE_SOURCES: usize = 32;

/// A Google Cloud Storage backend for tus uploads.
///
/// Internally uses the official [`google-cloud-storage`] SDK (gRPC).  The
/// client structs hold connection pools behind an `Arc`, so cloning is cheap.
///
/// # Authentication
///
/// Uses Application Default Credentials by default. Override with
/// `GcsStoreBuilder::with_storage` / `with_control` for custom credentials or
/// endpoints.
pub struct GcsStore {
    storage: Storage,
    control: StorageControl,
    /// `projects/_/buckets/{bucket}` format, ready for API calls.
    bucket: String,
    /// Object-name prefix (may be empty). Always ends with `/` if non-empty.
    prefix: String,
}

impl std::fmt::Debug for GcsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcsStore")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

// --- Construction -----------------------------------------------------------

/// Builder for [`GcsStore`].
///
/// ```no_run
/// # async fn demo() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// use fileloft_store_gcs::GcsStore;
///
/// let store = GcsStore::builder("my-bucket")
///     .prefix("uploads/")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct GcsStoreBuilder {
    bucket: String,
    prefix: String,
    storage: Option<Storage>,
    control: Option<StorageControl>,
}

impl GcsStoreBuilder {
    /// Set an object-name prefix (e.g. `"uploads/"`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Provide a pre-configured [`Storage`] client (data-plane).
    pub fn with_storage(mut self, s: Storage) -> Self {
        self.storage = Some(s);
        self
    }

    /// Provide a pre-configured [`StorageControl`] client (control-plane).
    pub fn with_control(mut self, c: StorageControl) -> Self {
        self.control = Some(c);
        self
    }

    /// Build the store, creating SDK clients if none were provided.
    pub async fn build(self) -> Result<GcsStore, GcsStoreError> {
        if self.bucket.is_empty() {
            return Err(GcsStoreError::BucketEmpty);
        }

        let storage = match self.storage {
            Some(s) => s,
            None => Storage::builder()
                .build()
                .await
                .map_err(|e| GcsStoreError::ClientInit(e.to_string()))?,
        };
        let control = match self.control {
            Some(c) => c,
            None => StorageControl::builder()
                .build()
                .await
                .map_err(|e| GcsStoreError::ClientInit(e.to_string()))?,
        };

        let mut prefix = self.prefix;
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        Ok(GcsStore {
            storage,
            control,
            bucket: format!("projects/_/buckets/{}", self.bucket),
            prefix,
        })
    }
}

impl GcsStore {
    /// Start building a new [`GcsStore`] for the given bucket name.
    pub fn builder(bucket: impl Into<String>) -> GcsStoreBuilder {
        GcsStoreBuilder {
            bucket: bucket.into(),
            prefix: String::new(),
            storage: None,
            control: None,
        }
    }
}

// --- Low-level GCS operations -----------------------------------------------

impl GcsStore {
    fn info_key(&self, id: &str) -> String {
        format!("{}{}.info", self.prefix, id)
    }

    async fn write_info_object(&self, info: &UploadInfo) -> Result<(), TusError> {
        let key = self.info_key(info.id.as_str());
        let id = info.id.as_str();
        let json = serde_json::to_vec_pretty(info)
            .map_err(|e| TusError::Internal(format!("serialize info: {e}")))?;

        self.storage
            .write_object(&self.bucket, &key, Bytes::from(json))
            .send_buffered()
            .await
            .map_err(|e| gcs_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_object(&self, id: &str) -> Result<UploadInfo, TusError> {
        let key = self.info_key(id);

        let mut resp = self
            .storage
            .read_object(&self.bucket, &key)
            .send()
            .await
            .map_err(|e| gcs_err(e, id, "read info"))?;

        let mut buf = Vec::new();
        while let Some(chunk) = resp.next().await {
            let chunk = chunk.map_err(|e| TusError::Internal(format!("GCS read chunk: {e}")))?;
            buf.extend_from_slice(&chunk);
        }

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }
}

// --- DataStore impl ---------------------------------------------------------

impl SendDataStore for GcsStore {
    type UploadType = GcsUpload;

    #[instrument(skip(self, info), fields(upload_id = %info.id))]
    async fn create_upload(&self, info: UploadInfo) -> Result<GcsUpload, TusError> {
        self.write_info_object(&info).await?;
        debug!("created upload");

        Ok(GcsUpload {
            id: info.id.clone(),
            storage: self.storage.clone(),
            control: self.control.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
        })
    }

    #[instrument(skip(self))]
    async fn get_upload(&self, id: &UploadId) -> Result<GcsUpload, TusError> {
        // Verify the upload exists by reading its .info object.
        let _info = self.read_info_object(id.as_str()).await?;

        Ok(GcsUpload {
            id: id.clone(),
            storage: self.storage.clone(),
            control: self.control.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
        })
    }
}

// --- Upload handle ----------------------------------------------------------

/// Handle to a single GCS-backed upload.
pub struct GcsUpload {
    id: UploadId,
    storage: Storage,
    control: StorageControl,
    bucket: String,
    prefix: String,
}

impl GcsUpload {
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

        self.storage
            .write_object(&self.bucket, &key, Bytes::from(json))
            .send_buffered()
            .await
            .map_err(|e| gcs_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_object(&self) -> Result<UploadInfo, TusError> {
        let key = self.info_key();
        let id = self.id.as_str();

        let mut resp = self
            .storage
            .read_object(&self.bucket, &key)
            .send()
            .await
            .map_err(|e| gcs_err(e, id, "read info"))?;

        let mut buf = Vec::new();
        while let Some(chunk) = resp.next().await {
            let chunk = chunk.map_err(|e| TusError::Internal(format!("GCS read chunk: {e}")))?;
            buf.extend_from_slice(&chunk);
        }

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }

    async fn list_parts(&self) -> Result<Vec<String>, TusError> {
        let id = self.id.as_str();
        let part_prefix = format!("{}{}_part_", self.prefix, id);
        let mut names = Vec::new();
        let mut page_token = String::new();

        loop {
            let mut req = self
                .control
                .list_objects()
                .set_parent(&self.bucket)
                .set_prefix(&part_prefix)
                .set_page_size(1000);
            if !page_token.is_empty() {
                req = req.set_page_token(&page_token);
            }
            let page = req.send().await.map_err(|e| gcs_err(e, id, "list parts"))?;
            for obj in &page.objects {
                if !obj.name.is_empty() {
                    names.push(obj.name.clone());
                }
            }
            page_token = page.next_page_token.clone();
            if page_token.is_empty() {
                break;
            }
        }

        names.sort_by(|a, b| part_index(a).cmp(&part_index(b)));
        Ok(names)
    }

    async fn compose_chunk(&self, dest_key: &str, source_keys: &[String]) -> Result<(), TusError> {
        if source_keys.is_empty() {
            return Ok(());
        }

        let source_objects: Vec<SourceObject> = source_keys
            .iter()
            .map(|name| SourceObject::new().set_name(name.clone()))
            .collect();

        let dest = Object::new().set_bucket(&self.bucket).set_name(dest_key);

        self.control
            .compose_object()
            .set_destination(dest)
            .set_source_objects(source_objects)
            .set_delete_source_objects(true)
            .send()
            .await
            .map_err(|e| gcs_err(e, self.id.as_str(), "compose"))?;
        Ok(())
    }
}

impl SendUpload for GcsUpload {
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

        let end_offset = offset
            .checked_add(n)
            .ok_or_else(|| TusError::Internal("upload offset overflow".into()))?;
        if let Some(declared) = info.size {
            if end_offset > declared {
                return Err(TusError::ExceedsUploadLength {
                    declared,
                    end: end_offset,
                });
            }
        }

        // Write this chunk as a numbered part object.
        let part_index = self.list_parts().await?.len() as u32;
        let part_key = self.part_key(part_index);

        self.storage
            .write_object(&self.bucket, &part_key, Bytes::from(buf))
            .send_buffered()
            .await
            .map_err(|e| gcs_err(e, self.id.as_str(), "write part"))?;

        info.offset = end_offset;
        self.write_info_object(&info).await?;

        debug!(bytes = n, new_offset = end_offset, "wrote chunk");
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        self.read_info_object().await
    }

    async fn read_content(&self) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, TusError> {
        let info = self.read_info_object().await?;
        if !info.is_complete() {
            return Err(TusError::UploadNotReadyForDownload);
        }
        let key = self.data_key();
        let mut resp = self
            .storage
            .read_object(&self.bucket, &key)
            .send()
            .await
            .map_err(|e| gcs_err(e, self.id.as_str(), "read download"))?;
        let mut buf = Vec::new();
        while let Some(chunk) = resp.next().await {
            let chunk = chunk.map_err(|e| TusError::Internal(format!("GCS read download: {e}")))?;
            buf.extend_from_slice(&chunk);
        }
        Ok(Box::new(Cursor::new(buf)))
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn finalize(&mut self) -> Result<(), TusError> {
        let parts = self.list_parts().await?;
        if parts.is_empty() {
            // Zero-length upload: write an empty final object.
            self.storage
                .write_object(&self.bucket, &self.data_key(), Bytes::new())
                .send_buffered()
                .await
                .map_err(|e| TusError::Internal(format!("GCS write empty final: {e}")))?;
            return Ok(());
        }

        let dest_key = self.data_key();
        if parts.len() <= MAX_COMPOSE_SOURCES {
            self.compose_chunk(&dest_key, &parts).await?;
        } else {
            let mut intermediates = Vec::new();
            for (i, chunk) in parts.chunks(MAX_COMPOSE_SOURCES).enumerate() {
                let tmp_key = format!("{}{}_tmp_{}", self.prefix, self.id.as_str(), i);
                self.compose_chunk(&tmp_key, &chunk.to_vec()).await?;
                intermediates.push(tmp_key);
            }
            self.compose_chunk(&dest_key, &intermediates).await?;
        }

        debug!("finalized upload");
        Ok(())
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn delete(self) -> Result<(), TusError> {
        let delete = |key: String| {
            let control = self.control.clone();
            let bucket = self.bucket.clone();
            async move {
                let _ = control
                    .delete_object()
                    .set_bucket(&bucket)
                    .set_object(&key)
                    .send()
                    .await;
            }
        };

        delete(self.data_key()).await;
        delete(self.info_key()).await;

        let parts = self.list_parts().await.unwrap_or_default();
        for part_key in parts {
            delete(part_key).await;
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
        // Each partial's composed data object is {prefix}{partial_id}.
        let source_keys: Vec<String> = partials
            .iter()
            .map(|p| format!("{}{}", self.prefix, p.id.as_str()))
            .collect();

        let dest_key = self.data_key();

        if source_keys.len() <= MAX_COMPOSE_SOURCES {
            self.compose_chunk(&dest_key, &source_keys).await?;
        } else {
            let mut intermediates = Vec::new();
            for (i, chunk) in source_keys.chunks(MAX_COMPOSE_SOURCES).enumerate() {
                let tmp_key = format!("{}{}_tmp_{}", self.prefix, self.id.as_str(), i);
                self.compose_chunk(&tmp_key, &chunk.to_vec()).await?;
                intermediates.push(tmp_key);
            }
            self.compose_chunk(&dest_key, &intermediates).await?;
        }

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

/// Check whether a GCS SDK error represents a 404 / NOT_FOUND.
///
/// The SDK can surface errors via gRPC `Status` (with a `Code`) or via raw
/// HTTP (with a status code).  We check both paths.
fn is_not_found(e: &google_cloud_storage::Error) -> bool {
    if e.http_status_code() == Some(404) {
        return true;
    }
    if let Some(status) = e.status() {
        // Code::NotFound == 5, matching google_cloud_gax::error::rpc::Code.
        return status.code as i32 == 5;
    }
    false
}

/// Convert a GCS SDK error into a [`TusError`], mapping 404s to `NotFound`.
fn gcs_err(e: google_cloud_storage::Error, upload_id: &str, op: &str) -> TusError {
    if is_not_found(&e) {
        TusError::NotFound(upload_id.to_string())
    } else {
        error!(%e, op, "GCS operation failed");
        TusError::Internal(format!("GCS {op}: {e}"))
    }
}
