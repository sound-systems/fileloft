use std::path::PathBuf;

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};

/// Each upload lives in `<root>/<upload_id>/` with `info.json` and `data`.
#[derive(Clone, Debug)]
pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn upload_dir(&self, id: &UploadId) -> PathBuf {
        self.root.join(id.as_str())
    }

    fn info_path(&self, id: &UploadId) -> PathBuf {
        self.upload_dir(id).join("info.json")
    }

    fn data_path(&self, id: &UploadId) -> PathBuf {
        self.upload_dir(id).join("data")
    }
}

impl SendDataStore for FileStore {
    type UploadType = FileUpload;

    async fn create_upload(&self, info: UploadInfo) -> Result<FileUpload, TusError> {
        let dir = self.upload_dir(&info.id);
        tokio::fs::create_dir_all(&dir).await.map_err(TusError::Io)?;
        let json = serde_json::to_vec(&info).map_err(|e| TusError::Internal(e.to_string()))?;
        tokio::fs::write(self.info_path(&info.id), &json)
            .await
            .map_err(TusError::Io)?;
        tokio::fs::write(self.data_path(&info.id), &[])
            .await
            .map_err(TusError::Io)?;
        Ok(FileUpload {
            root: self.root.clone(),
            id: info.id.clone(),
        })
    }

    async fn get_upload(&self, id: &UploadId) -> Result<FileUpload, TusError> {
        let dir = self.upload_dir(id);
        if !tokio::fs::try_exists(&dir).await.map_err(TusError::Io)? {
            return Err(TusError::NotFound(id.to_string()));
        }
        Ok(FileUpload {
            root: self.root.clone(),
            id: id.clone(),
        })
    }
}

pub struct FileUpload {
    root: PathBuf,
    id: UploadId,
}

impl FileUpload {
    fn info_path(&self) -> PathBuf {
        self.root.join(self.id.as_str()).join("info.json")
    }

    fn data_path(&self) -> PathBuf {
        self.root.join(self.id.as_str()).join("data")
    }

    async fn read_info(&self) -> Result<UploadInfo, TusError> {
        let bytes = tokio::fs::read(&self.info_path())
            .await
            .map_err(TusError::Io)?;
        serde_json::from_slice(&bytes).map_err(|e| TusError::Internal(e.to_string()))
    }

    async fn write_info(&self, info: &UploadInfo) -> Result<(), TusError> {
        let json = serde_json::to_vec(info).map_err(|e| TusError::Internal(e.to_string()))?;
        tokio::fs::write(&self.info_path(), &json)
            .await
            .map_err(TusError::Io)
    }
}

impl SendUpload for FileUpload {
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        let n = buf.len() as u64;
        let mut info = self.read_info().await?;
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

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.data_path())
            .await
            .map_err(TusError::Io)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(TusError::Io)?;
        file.write_all(&buf).await.map_err(TusError::Io)?;
        info.offset = offset + n;
        self.write_info(&info).await?;
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        self.read_info().await
    }

    async fn finalize(&mut self) -> Result<(), TusError> {
        Ok(())
    }

    async fn delete(self) -> Result<(), TusError> {
        let dir = self.root.join(self.id.as_str());
        if tokio::fs::try_exists(&dir).await.map_err(TusError::Io)? {
            tokio::fs::remove_dir_all(&dir).await.map_err(TusError::Io)?;
        }
        Ok(())
    }

    async fn declare_length(&mut self, length: u64) -> Result<(), TusError> {
        let mut info = self.read_info().await?;
        if info.size.is_some() {
            return Err(TusError::UploadLengthAlreadySet);
        }
        info.size = Some(length);
        info.size_is_deferred = false;
        self.write_info(&info).await
    }

    async fn concatenate(&mut self, partials: &[UploadInfo]) -> Result<(), TusError> {
        let mut combined = Vec::new();
        for partial in partials {
            let path = self.root.join(partial.id.as_str()).join("data");
            let chunk = tokio::fs::read(&path).await.map_err(TusError::Io)?;
            combined.extend_from_slice(&chunk);
        }
        let total = combined.len() as u64;
        tokio::fs::write(&self.data_path(), &combined)
            .await
            .map_err(TusError::Io)?;
        let mut info = self.read_info().await?;
        info.size = Some(total);
        info.offset = total;
        info.is_final = true;
        self.write_info(&info).await
    }
}
