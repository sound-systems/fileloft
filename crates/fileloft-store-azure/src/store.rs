//! Azure Blob Storage backend for fileloft.
//!
//! Blob layout (same logical keys as GCS):
//!
//! - Metadata: `{prefix}{id}.info` — JSON-serialised [`UploadInfo`].
//! - Parts: `{prefix}{id}_part_{n}` — one blob per PATCH chunk.
//! - Final: `{prefix}{id}` — block blob built from part blobs on finalize.

use azure_core::{request_options::Prefix, Body, StatusCode};
use azure_identity::create_default_credential;
use azure_storage::{ConnectionString, StorageCredentials};
use azure_storage_blobs::prelude::*;
use bytes::Bytes;
use futures::StreamExt;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, instrument};

use crate::error::AzureStoreError;
use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};

/// Maximum number of source blobs committed together in one pass (mirrors GCS compose batching).
const MAX_COMMIT_SOURCES: usize = 32;

/// Azure Blob Storage backend for tus uploads.
///
/// Uses the official [`azure_storage_blobs`] SDK with Tokio. Authenticate via
/// `AZURE_STORAGE_CONNECTION_STRING`, or `AZURE_STORAGE_ACCOUNT` plus
/// [`create_default_credential`](azure_identity::create_default_credential), or inject a
/// [`BlobServiceClient`] with [`AzureStoreBuilder::with_blob_service_client`].
#[derive(Clone)]
pub struct AzureStore {
    container: ContainerClient,
    prefix: String,
}

impl std::fmt::Debug for AzureStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureStore")
            .field("container", &self.container.container_name())
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// Builder for [`AzureStore`].
///
/// ```no_run
/// # async fn demo() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// use fileloft_store_azure::AzureStore;
///
/// let store = AzureStore::builder("my-container")
///     .prefix("uploads/")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct AzureStoreBuilder {
    container: String,
    prefix: String,
    service: Option<BlobServiceClient>,
    connection_string: Option<String>,
    account: Option<String>,
}

impl AzureStoreBuilder {
    /// Set a blob name prefix (e.g. `"uploads/"`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Use a pre-built [`BlobServiceClient`] (account + credentials).
    pub fn with_blob_service_client(mut self, service: BlobServiceClient) -> Self {
        self.service = Some(service);
        self
    }

    /// Use this connection string instead of `AZURE_STORAGE_CONNECTION_STRING`.
    pub fn connection_string(mut self, s: impl Into<String>) -> Self {
        self.connection_string = Some(s.into());
        self
    }

    /// Storage account name when not using a connection string (falls back to `AZURE_STORAGE_ACCOUNT`).
    pub fn account(mut self, account: impl Into<String>) -> Self {
        self.account = Some(account.into());
        self
    }

    /// Build the store.
    pub async fn build(self) -> Result<AzureStore, AzureStoreError> {
        if self.container.trim().is_empty() {
            return Err(AzureStoreError::ContainerEmpty);
        }

        let mut prefix = self.prefix;
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        let service = if let Some(s) = self.service {
            s
        } else if let Some(cs) = self
            .connection_string
            .or_else(|| std::env::var("AZURE_STORAGE_CONNECTION_STRING").ok())
        {
            let cs = ConnectionString::new(&cs)
                .map_err(|e| AzureStoreError::ClientInit(e.to_string()))?;
            let account = cs
                .account_name
                .ok_or_else(|| AzureStoreError::ClientInit("connection string missing AccountName".into()))?;
            let creds = cs
                .storage_credentials()
                .map_err(|e| AzureStoreError::ClientInit(e.to_string()))?;
            BlobServiceClient::new(account, creds)
        } else {
            let account = self
                .account
                .or_else(|| std::env::var("AZURE_STORAGE_ACCOUNT").ok())
                .filter(|s| !s.trim().is_empty())
                .ok_or(AzureStoreError::AccountMissing)?;
            let token = create_default_credential()
                .map_err(|e| AzureStoreError::ClientInit(e.to_string()))?;
            BlobServiceClient::new(account, StorageCredentials::token_credential(token))
        };

        let container = service.container_client(self.container);
        Ok(AzureStore { container, prefix })
    }
}

impl AzureStore {
    /// Start building an [`AzureStore`] for the given container name.
    pub fn builder(container: impl Into<String>) -> AzureStoreBuilder {
        AzureStoreBuilder {
            container: container.into(),
            prefix: String::new(),
            service: None,
            connection_string: None,
            account: None,
        }
    }

    fn info_key(&self, id: &str) -> String {
        format!("{}{}.info", self.prefix, id)
    }

    async fn write_info_blob(&self, info: &UploadInfo) -> Result<(), TusError> {
        let key = self.info_key(info.id.as_str());
        let id = info.id.as_str();
        let json = serde_json::to_vec_pretty(info)
            .map_err(|e| TusError::Internal(format!("serialize info: {e}")))?;

        self.container
            .blob_client(&key)
            .put_block_blob(Body::from(json))
            .await
            .map_err(|e| azure_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_blob(&self, id: &str) -> Result<UploadInfo, TusError> {
        let key = self.info_key(id);
        let blob = self.container.blob_client(&key);
        let buf = blob
            .get_content()
            .await
            .map_err(|e| azure_err(e, id, "read info"))?;

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }
}

impl SendDataStore for AzureStore {
    type UploadType = AzureUpload;

    #[instrument(skip(self, info), fields(upload_id = %info.id))]
    async fn create_upload(&self, info: UploadInfo) -> Result<AzureUpload, TusError> {
        self.write_info_blob(&info).await?;
        debug!("created upload");

        Ok(AzureUpload {
            id: info.id.clone(),
            container: self.container.clone(),
            prefix: self.prefix.clone(),
        })
    }

    #[instrument(skip(self))]
    async fn get_upload(&self, id: &UploadId) -> Result<AzureUpload, TusError> {
        let _info = self.read_info_blob(id.as_str()).await?;

        Ok(AzureUpload {
            id: id.clone(),
            container: self.container.clone(),
            prefix: self.prefix.clone(),
        })
    }
}

/// Handle to a single Azure-backed upload.
#[derive(Clone)]
pub struct AzureUpload {
    id: UploadId,
    container: ContainerClient,
    prefix: String,
}

impl AzureUpload {
    fn data_key(&self) -> String {
        format!("{}{}", self.prefix, self.id.as_str())
    }

    fn info_key(&self) -> String {
        format!("{}{}.info", self.prefix, self.id.as_str())
    }

    fn part_key(&self, index: u32) -> String {
        format!("{}{}_part_{}", self.prefix, self.id.as_str(), index)
    }

    async fn write_info_blob(&self, info: &UploadInfo) -> Result<(), TusError> {
        let key = self.info_key();
        let id = self.id.as_str();
        let json = serde_json::to_vec_pretty(info)
            .map_err(|e| TusError::Internal(format!("serialize info: {e}")))?;

        self.container
            .blob_client(&key)
            .put_block_blob(Body::from(json))
            .await
            .map_err(|e| azure_err(e, id, "write info"))?;
        Ok(())
    }

    async fn read_info_blob(&self) -> Result<UploadInfo, TusError> {
        let key = self.info_key();
        let id = self.id.as_str();
        let buf = self
            .container
            .blob_client(&key)
            .get_content()
            .await
            .map_err(|e| azure_err(e, id, "read info"))?;

        serde_json::from_slice(&buf)
            .map_err(|e| TusError::Internal(format!("deserialize info: {e}")))
    }

    async fn list_parts(&self) -> Result<Vec<String>, TusError> {
        let id = self.id.as_str();
        let part_prefix = format!("{}{}_part_", self.prefix, id);
        let mut names = Vec::new();

        let mut stream = self
            .container
            .list_blobs()
            .prefix(Prefix::new(part_prefix.clone()))
            .into_stream();

        while let Some(page) = stream.next().await {
            let page = page.map_err(|e| azure_err(e, id, "list parts"))?;
            for blob in page.blobs.blobs() {
                if !blob.name.is_empty() {
                    names.push(blob.name.clone());
                }
            }
        }

        names.sort_by_key(|a| part_index(a));
        Ok(names)
    }

    async fn delete_blob_best_effort(&self, blob_name: &str) {
        let _ = self
            .container
            .blob_client(blob_name)
            .delete()
            .await;
    }

    /// Stage `source_blob_names` into a new block blob at `dest_blob_name`, then delete sources.
    async fn commit_and_delete_sources(
        &self,
        dest_blob_name: &str,
        source_blob_names: &[String],
        op: &str,
    ) -> Result<(), TusError> {
        commit_blob_names_to_dest(
            &self.container,
            dest_blob_name,
            source_blob_names,
            self.id.as_str(),
            op,
        )
        .await?;
        for name in source_blob_names {
            self.delete_blob_best_effort(name.as_str()).await;
        }
        Ok(())
    }

    async fn commit_many_blobs(
        &self,
        dest_blob_name: &str,
        mut sources: Vec<String>,
        op: &str,
    ) -> Result<(), TusError> {
        if sources.is_empty() {
            return Ok(());
        }

        let mut tmp_counter: u32 = 0;
        while sources.len() > MAX_COMMIT_SOURCES {
            let chunks: Vec<Vec<String>> = sources
                .chunks(MAX_COMMIT_SOURCES)
                .map(|c| c.to_vec())
                .collect();
            let mut next = Vec::new();
            for chunk in chunks {
                let tmp_key = format!("{}{}_tmp_{}", self.prefix, self.id.as_str(), tmp_counter);
                tmp_counter = tmp_counter.saturating_add(1);
                self.commit_and_delete_sources(&tmp_key, &chunk, op).await?;
                next.push(tmp_key);
            }
            sources = next;
        }

        self.commit_and_delete_sources(dest_blob_name, &sources, op)
            .await
    }
}

impl SendUpload for AzureUpload {
    #[instrument(skip(self, reader), fields(upload_id = %self.id))]
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        let mut info = self.read_info_blob().await?;
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

        self.container
            .blob_client(&part_key)
            .put_block_blob(Body::from(buf))
            .await
            .map_err(|e| azure_err(e, self.id.as_str(), "write part"))?;

        info.offset = end_offset;
        self.write_info_blob(&info).await?;

        debug!(bytes = n, new_offset = end_offset, "wrote chunk");
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        self.read_info_blob().await
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn finalize(&mut self) -> Result<(), TusError> {
        let parts = self.list_parts().await?;
        let dest_key = self.data_key();

        if parts.is_empty() {
            self.container
                .blob_client(&dest_key)
                .put_block_blob(Body::from(Bytes::new()))
                .await
                .map_err(|e| azure_err(e, self.id.as_str(), "write empty final"))?;
            return Ok(());
        }

        self.commit_many_blobs(&dest_key, parts, "finalize")
            .await?;

        debug!("finalized upload");
        Ok(())
    }

    #[instrument(skip(self), fields(upload_id = %self.id))]
    async fn delete(self) -> Result<(), TusError> {
        self.delete_blob_best_effort(&self.data_key()).await;
        self.delete_blob_best_effort(&self.info_key()).await;

        let parts = self.list_parts().await.unwrap_or_default();
        for part_key in parts {
            self.delete_blob_best_effort(&part_key).await;
        }

        debug!("terminated upload");
        Ok(())
    }

    async fn declare_length(&mut self, length: u64) -> Result<(), TusError> {
        let mut info = self.read_info_blob().await?;
        if info.size.is_some() {
            return Err(TusError::UploadLengthAlreadySet);
        }
        info.size = Some(length);
        info.size_is_deferred = false;
        self.write_info_blob(&info).await
    }

    #[instrument(skip(self, partials), fields(upload_id = %self.id, n_partials = partials.len()))]
    async fn concatenate(&mut self, partials: &[UploadInfo]) -> Result<(), TusError> {
        let source_keys: Vec<String> = partials
            .iter()
            .map(|p| format!("{}{}", self.prefix, p.id.as_str()))
            .collect();

        let dest_key = self.data_key();
        self.commit_many_blobs(&dest_key, source_keys, "concatenate")
            .await?;

        let total: u64 = partials.iter().filter_map(|p| p.size).sum();
        let mut info = self.read_info_blob().await?;
        info.size = Some(total);
        info.offset = total;
        info.is_final = true;
        self.write_info_blob(&info).await?;

        debug!("concatenated {} partials", partials.len());
        Ok(())
    }
}

fn block_id_for_index(i: u64) -> BlockId {
    BlockId::new(Bytes::copy_from_slice(&i.to_be_bytes()))
}

async fn commit_blob_names_to_dest(
    container: &ContainerClient,
    dest_blob_name: &str,
    source_blob_names: &[String],
    upload_id: &str,
    op: &str,
) -> Result<(), TusError> {
    if source_blob_names.is_empty() {
        return Ok(());
    }

    let dest = container.blob_client(dest_blob_name);
    let mut block_list = BlockList::default();

    for (i, src_name) in source_blob_names.iter().enumerate() {
        let data = container
            .blob_client(src_name)
            .get_content()
            .await
            .map_err(|e| azure_err(e, upload_id, op))?;

        let block_id = block_id_for_index(i as u64);
        dest
            .put_block(block_id.clone(), Body::from(data))
            .await
            .map_err(|e| azure_err(e, upload_id, op))?;
        block_list
            .blocks
            .push(BlobBlockType::new_uncommitted(block_id));
    }

    dest.put_block_list(block_list)
        .await
        .map_err(|e| azure_err(e, upload_id, op))?;

    Ok(())
}

pub(crate) fn part_index(key: &str) -> u32 {
    key.rsplit('_')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

fn is_not_found(e: &azure_core::Error) -> bool {
    e.as_http_error()
        .map(|h| h.status() == StatusCode::NotFound)
        .unwrap_or(false)
}

fn azure_err(e: azure_core::Error, upload_id: &str, op: &str) -> TusError {
    if is_not_found(&e) {
        TusError::NotFound(upload_id.to_string())
    } else {
        error!(%e, op, "Azure Blob operation failed");
        TusError::Internal(format!("Azure {op}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::part_index;

    #[test]
    fn part_index_parses_suffix() {
        assert_eq!(part_index("uploads/abc_part_0"), 0);
        assert_eq!(part_index("uploads/abc_part_12"), 12);
        assert_eq!(part_index("nounderscore"), 0);
    }
}
