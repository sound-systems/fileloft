use std::collections::HashMap;
use std::sync::Arc;

use bytes::BytesMut;
use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;

#[derive(Debug)]
struct MemoryUploadState {
    info: UploadInfo,
    data: BytesMut,
}

type StoreMap = Arc<RwLock<HashMap<String, MemoryUploadState>>>;

/// An in-memory tus storage backend.
///
/// Data is held in a shared `Arc<RwLock<HashMap>>` so all handles and clones
/// refer to the same underlying state. Suitable for testing and development;
/// not intended for production use (no persistence, no multi-process support).
#[derive(Clone)]
pub struct MemoryStore {
    state: StoreMap,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SendDataStore for MemoryStore {
    type UploadType = MemoryUpload;

    async fn create_upload(&self, info: UploadInfo) -> Result<MemoryUpload, TusError> {
        let id = info.id.clone();
        let mut state = self.state.write().await;
        state.insert(
            id.as_str().to_string(),
            MemoryUploadState {
                info,
                data: BytesMut::new(),
            },
        );
        Ok(MemoryUpload {
            id,
            store: Arc::clone(&self.state),
        })
    }

    async fn get_upload(&self, id: &UploadId) -> Result<MemoryUpload, TusError> {
        let state = self.state.read().await;
        if state.contains_key(id.as_str()) {
            Ok(MemoryUpload {
                id: id.clone(),
                store: Arc::clone(&self.state),
            })
        } else {
            Err(TusError::NotFound(id.to_string()))
        }
    }
}

/// Handle to a single in-memory upload.
pub struct MemoryUpload {
    id: UploadId,
    store: StoreMap,
}

impl SendUpload for MemoryUpload {
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        let n = buf.len() as u64;

        let mut state = self.store.write().await;
        let entry = state
            .get_mut(self.id.as_str())
            .ok_or_else(|| TusError::NotFound(self.id.to_string()))?;

        let end_offset = offset
            .checked_add(n)
            .ok_or_else(|| TusError::Internal("upload offset overflow".into()))?;
        if let Some(declared) = entry.info.size {
            if end_offset > declared {
                return Err(TusError::ExceedsUploadLength {
                    declared,
                    end: end_offset,
                });
            }
        }

        // Ensure the buffer is large enough
        let end = (offset + n) as usize;
        if entry.data.len() < end {
            entry.data.resize(end, 0);
        }
        entry.data[offset as usize..end].copy_from_slice(&buf);
        entry.info.offset = offset + n;
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        let state = self.store.read().await;
        state
            .get(self.id.as_str())
            .map(|s| s.info.clone())
            .ok_or_else(|| TusError::NotFound(self.id.to_string()))
    }

    async fn finalize(&mut self) -> Result<(), TusError> {
        // In-memory: nothing to do (already committed on write_chunk)
        Ok(())
    }

    async fn delete(self) -> Result<(), TusError> {
        let mut state = self.store.write().await;
        state
            .remove(self.id.as_str())
            .ok_or_else(|| TusError::NotFound(self.id.to_string()))?;
        Ok(())
    }

    async fn declare_length(&mut self, length: u64) -> Result<(), TusError> {
        let mut state = self.store.write().await;
        let entry = state
            .get_mut(self.id.as_str())
            .ok_or_else(|| TusError::NotFound(self.id.to_string()))?;
        if entry.info.size.is_some() {
            return Err(TusError::UploadLengthAlreadySet);
        }
        entry.info.size = Some(length);
        entry.info.size_is_deferred = false;
        Ok(())
    }

    async fn concatenate(&mut self, partials: &[UploadInfo]) -> Result<(), TusError> {
        // Collect data from each partial upload
        let mut combined = BytesMut::new();
        {
            let state = self.store.read().await;
            for partial in partials {
                let entry = state
                    .get(partial.id.as_str())
                    .ok_or_else(|| TusError::NotFound(partial.id.to_string()))?;
                combined.extend_from_slice(&entry.data);
            }
        }

        let total = combined.len() as u64;
        let mut state = self.store.write().await;
        let entry = state
            .get_mut(self.id.as_str())
            .ok_or_else(|| TusError::NotFound(self.id.to_string()))?;
        entry.data = combined;
        entry.info.size = Some(total);
        entry.info.offset = total;
        entry.info.is_final = true;
        Ok(())
    }
}

/// Retrieve the raw bytes of a completed upload (useful in tests).
pub async fn get_upload_data(store: &MemoryStore, id: &UploadId) -> Option<bytes::Bytes> {
    let state = store.state.read().await;
    state
        .get(id.as_str())
        .map(|s| bytes::Bytes::copy_from_slice(&s.data))
}
