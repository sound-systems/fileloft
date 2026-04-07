//! Filesystem tus.io storage backend.
//!
//! On-disk layout matches tusd / GCS-style keys (relative to `root`):
//!
//! - Metadata: `{root}/{prefix}{id}.info` — JSON [`UploadInfo`].
//! - Parts: `{root}/{prefix}{id}_part_{n}` — one file per PATCH chunk.
//! - Final: `{root}/{prefix}{id}` — assembled from parts in [`SendUpload::finalize`].
//!
//! `prefix` may contain path segments (e.g. `uploads/`); parent directories are created as needed.

use std::path::{Path, PathBuf};

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use fileloft_core::{
    error::TusError,
    info::{UploadId, UploadInfo},
    store::{SendDataStore, SendUpload},
};

/// Filesystem-backed store using a flat tusd-style key layout under `root`.
#[derive(Clone, Debug)]
pub struct FileStore {
    root: PathBuf,
    /// Object-key prefix; empty or ends with `/` (normalized by [`FileStore::with_prefix`]).
    prefix: String,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            prefix: String::new(),
        }
    }

    /// Same as [`FileStore::new`], with an object-key prefix such as `"uploads/"`.
    ///
    /// A non-empty prefix is normalized to end with `/`, matching GCS object naming.
    pub fn with_prefix(root: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        let mut prefix = prefix.into();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        Self {
            root: root.into(),
            prefix,
        }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        let mut p = self.root.clone();
        for seg in key.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }

    fn object_key_info(&self, id: &UploadId) -> String {
        format!("{}{}.info", self.prefix, id.as_str())
    }

    fn info_path(&self, id: &UploadId) -> PathBuf {
        self.key_to_path(&self.object_key_info(id))
    }
}

impl SendDataStore for FileStore {
    type UploadType = FileUpload;

    async fn create_upload(&self, info: UploadInfo) -> Result<FileUpload, TusError> {
        let path = self.info_path(&info.id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(TusError::Io)?;
        }
        let json = serde_json::to_vec(&info).map_err(|e| TusError::Internal(e.to_string()))?;
        tokio::fs::write(&path, &json).await.map_err(TusError::Io)?;
        Ok(FileUpload {
            root: self.root.clone(),
            prefix: self.prefix.clone(),
            id: info.id.clone(),
        })
    }

    async fn get_upload(&self, id: &UploadId) -> Result<FileUpload, TusError> {
        let path = self.info_path(id);
        if !tokio::fs::try_exists(&path).await.map_err(TusError::Io)? {
            return Err(TusError::NotFound(id.to_string()));
        }
        Ok(FileUpload {
            root: self.root.clone(),
            prefix: self.prefix.clone(),
            id: id.clone(),
        })
    }
}

pub struct FileUpload {
    root: PathBuf,
    prefix: String,
    id: UploadId,
}

impl FileUpload {
    fn key_to_path(&self, key: &str) -> PathBuf {
        let mut p = self.root.clone();
        for seg in key.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }

    fn object_key_info(&self) -> String {
        format!("{}{}.info", self.prefix, self.id.as_str())
    }

    fn object_key_data(&self) -> String {
        format!("{}{}", self.prefix, self.id.as_str())
    }

    fn info_path(&self) -> PathBuf {
        self.key_to_path(&self.object_key_info())
    }

    fn data_path(&self) -> PathBuf {
        self.key_to_path(&self.object_key_data())
    }

    fn partial_data_path(&self, partial_id: &UploadId) -> PathBuf {
        let key = format!("{}{}", self.prefix, partial_id.as_str());
        self.key_to_path(&key)
    }

    fn part_path(&self, index: u32) -> PathBuf {
        let key = format!("{}{}_part_{}", self.prefix, self.id.as_str(), index);
        self.key_to_path(&key)
    }

    async fn read_info(&self) -> Result<UploadInfo, TusError> {
        let bytes = tokio::fs::read(self.info_path())
            .await
            .map_err(TusError::Io)?;
        serde_json::from_slice(&bytes).map_err(|e| TusError::Internal(e.to_string()))
    }

    async fn write_info(&self, info: &UploadInfo) -> Result<(), TusError> {
        let json = serde_json::to_vec(info).map_err(|e| TusError::Internal(e.to_string()))?;
        tokio::fs::write(self.info_path(), &json)
            .await
            .map_err(TusError::Io)
    }

    /// Lists part files for this upload, sorted by numeric `_part_{n}` suffix.
    async fn list_part_paths_sorted(&self) -> Result<Vec<PathBuf>, TusError> {
        let info_path = self.info_path();
        let Some(parent) = info_path.parent() else {
            return Ok(Vec::new());
        };
        if !tokio::fs::try_exists(parent).await.map_err(TusError::Io)? {
            return Ok(Vec::new());
        }

        let mut rd = tokio::fs::read_dir(parent).await.map_err(TusError::Io)?;
        let mut indexed: Vec<(u32, PathBuf)> = Vec::new();
        let name_prefix = format!("{}_part_", self.id.as_str());

        while let Some(ent) = rd.next_entry().await.map_err(TusError::Io)? {
            let name = ent.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            let Some(rest) = name_str.strip_prefix(&name_prefix) else {
                continue;
            };
            if let Ok(idx) = rest.parse::<u32>() {
                indexed.push((idx, ent.path()));
            }
        }

        indexed.sort_by_key(|(i, _)| *i);
        Ok(indexed.into_iter().map(|(_, p)| p).collect())
    }
}

async fn remove_file_ignore_not_found(path: &Path) -> Result<(), TusError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(TusError::Io(e)),
    }
}

impl SendUpload for FileUpload {
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        let mut info = self.read_info().await?;
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

        let part_index = self.list_part_paths_sorted().await?.len() as u32;
        let part_path = self.part_path(part_index);
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(TusError::Io)?;
        }
        tokio::fs::write(&part_path, &buf)
            .await
            .map_err(TusError::Io)?;

        info.offset = end_offset;
        self.write_info(&info).await?;
        Ok(n)
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        self.read_info().await
    }

    async fn finalize(&mut self) -> Result<(), TusError> {
        let parts = self.list_part_paths_sorted().await?;
        let dest = self.data_path();
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(TusError::Io)?;
        }

        if parts.is_empty() {
            tokio::fs::write(&dest, &[]).await.map_err(TusError::Io)?;
            return Ok(());
        }

        let mut out = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&dest)
            .await
            .map_err(TusError::Io)?;

        for part_path in &parts {
            let mut part_file = tokio::fs::File::open(part_path)
                .await
                .map_err(TusError::Io)?;
            tokio::io::copy(&mut part_file, &mut out)
                .await
                .map_err(TusError::Io)?;
        }
        out.flush().await.map_err(TusError::Io)?;

        for part_path in &parts {
            remove_file_ignore_not_found(part_path).await?;
        }
        Ok(())
    }

    async fn delete(self) -> Result<(), TusError> {
        remove_file_ignore_not_found(&self.data_path()).await?;
        remove_file_ignore_not_found(&self.info_path()).await?;
        let parts = self.list_part_paths_sorted().await?;
        for part in parts {
            remove_file_ignore_not_found(&part).await?;
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
        let dest = self.data_path();
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(TusError::Io)?;
        }

        let mut out = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&dest)
            .await
            .map_err(TusError::Io)?;

        for partial in partials {
            let src_path = self.partial_data_path(&partial.id);
            let mut src = tokio::fs::File::open(&src_path)
                .await
                .map_err(TusError::Io)?;
            tokio::io::copy(&mut src, &mut out)
                .await
                .map_err(TusError::Io)?;
        }
        out.flush().await.map_err(TusError::Io)?;

        let total: u64 = partials.iter().filter_map(|p| p.size).sum();
        let mut info = self.read_info().await?;
        info.size = Some(total);
        info.offset = total;
        info.is_final = true;
        self.write_info(&info).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fileloft_core::store::{SendDataStore, SendUpload};
    use std::io::Cursor;

    fn info_with_id(id: &str, size: Option<u64>) -> UploadInfo {
        UploadInfo::new(UploadId::from(id), size)
    }

    #[tokio::test]
    async fn create_patch_finalize_and_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::new(dir.path());
        let info = info_with_id("up-1", Some(11));

        let mut upload = store.create_upload(info).await.expect("create");

        let info_path = dir.path().join("up-1.info");
        assert!(tokio::fs::try_exists(&info_path).await.expect("exists"));

        let mut r = Cursor::new(b"hello ".as_slice());
        upload.write_chunk(0, &mut r).await.expect("chunk 1");
        let p0 = dir.path().join("up-1_part_0");
        assert!(tokio::fs::try_exists(&p0).await.expect("part 0"));

        let mut r = Cursor::new(b"world".as_slice());
        upload.write_chunk(6, &mut r).await.expect("chunk 2");

        upload.finalize().await.expect("finalize");

        let final_path = dir.path().join("up-1");
        let got = tokio::fs::read(&final_path).await.expect("read final");
        assert_eq!(got, b"hello world");
        assert!(!tokio::fs::try_exists(&p0).await.expect("part 0 gone"));
        assert!(!tokio::fs::try_exists(&dir.path().join("up-1_part_1"))
            .await
            .expect("stat"));
    }

    #[tokio::test]
    async fn zero_byte_upload_finalize() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::new(dir.path());
        let mut upload = store
            .create_upload(info_with_id("empty", Some(0)))
            .await
            .expect("create");
        upload.finalize().await.expect("finalize");
        let final_path = dir.path().join("empty");
        let got = tokio::fs::read(&final_path).await.expect("read");
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_final_info_and_parts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::new(dir.path());
        let mut upload = store
            .create_upload(info_with_id("del-me", Some(3)))
            .await
            .expect("create");
        let mut r = Cursor::new(b"abc".as_slice());
        upload.write_chunk(0, &mut r).await.expect("chunk");
        upload.finalize().await.expect("finalize");

        upload.delete().await.expect("delete");

        assert!(!tokio::fs::try_exists(dir.path().join("del-me"))
            .await
            .expect("stat"));
        assert!(!tokio::fs::try_exists(dir.path().join("del-me.info"))
            .await
            .expect("stat"));
    }

    #[tokio::test]
    async fn concatenate_builds_final_from_partial_finals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::new(dir.path());

        let mut p1 = store
            .create_upload(info_with_id("p1", Some(2)))
            .await
            .expect("p1");
        let mut r = Cursor::new(b"aa".as_slice());
        p1.write_chunk(0, &mut r).await.expect("w");
        p1.finalize().await.expect("f");

        let mut p2 = store
            .create_upload(info_with_id("p2", Some(2)))
            .await
            .expect("p2");
        let mut r = Cursor::new(b"bb".as_slice());
        p2.write_chunk(0, &mut r).await.expect("w");
        p2.finalize().await.expect("f");

        let mut fin = store
            .create_upload(info_with_id("final", None))
            .await
            .expect("final");
        let mut i1 = info_with_id("p1", Some(2));
        let mut i2 = info_with_id("p2", Some(2));
        i1.offset = 2;
        i2.offset = 2;
        fin.concatenate(&[i1, i2]).await.expect("concat");

        let got = tokio::fs::read(dir.path().join("final"))
            .await
            .expect("read");
        assert_eq!(got, b"aabb");
        let meta: UploadInfo = serde_json::from_slice(
            &tokio::fs::read(dir.path().join("final.info"))
                .await
                .expect("read info"),
        )
        .expect("json");
        assert_eq!(meta.offset, 4);
        assert_eq!(meta.size, Some(4));
        assert!(meta.is_final);
    }

    #[tokio::test]
    async fn with_prefix_creates_nested_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::with_prefix(dir.path(), "uploads");
        let mut upload = store
            .create_upload(info_with_id("x", Some(1)))
            .await
            .expect("create");
        let mut r = Cursor::new(b"z".as_slice());
        upload.write_chunk(0, &mut r).await.expect("chunk");
        assert!(tokio::fs::try_exists(dir.path().join("uploads/x_part_0"))
            .await
            .expect("stat"));
        upload.finalize().await.expect("finalize");
        assert!(tokio::fs::try_exists(dir.path().join("uploads/x"))
            .await
            .expect("stat"));
    }
}
