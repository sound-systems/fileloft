use std::path::PathBuf;
use std::time::Duration;

use fs4::fs_std::FileExt;
use fileloft_core::{
    error::TusError,
    info::UploadId,
    lock::{SendLock, SendLocker},
};

/// Directory for per-upload `*.lock` files (shared across processes using the same upload root).
#[derive(Clone, Debug)]
pub struct FileLocker {
    lock_dir: PathBuf,
    pub timeout: Duration,
}

impl FileLocker {
    pub fn new(lock_dir: impl Into<PathBuf>) -> Self {
        Self {
            lock_dir: lock_dir.into(),
            timeout: Duration::from_secs(20),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn lock_path(&self, id: &UploadId) -> PathBuf {
        self.lock_dir.join(format!("{}.lock", id.as_str()))
    }
}

impl SendLocker for FileLocker {
    type LockType = FileLock;

    async fn acquire(&self, id: &UploadId) -> Result<FileLock, TusError> {
        tokio::fs::create_dir_all(&self.lock_dir)
            .await
            .map_err(TusError::Io)?;
        let path = self.lock_path(id);
        let timeout = self.timeout;
        let id_str = id.to_string();

        tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                let f = std::fs::OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(TusError::Io)?;
                match f.try_lock_exclusive() {
                    Ok(true) => {
                        return Ok(FileLock {
                            file: Some(f),
                            path,
                        });
                    }
                    Ok(false) | Err(_) => {
                        drop(f);
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Err(TusError::LockTimeout(id_str));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .await
        .map_err(|e| TusError::Internal(format!("lock join: {e}")))?
    }
}

pub struct FileLock {
    file: Option<std::fs::File>,
    path: PathBuf,
}

impl SendLock for FileLock {
    async fn release(mut self) -> Result<(), TusError> {
        let path = self.path.clone();
        let file = self
            .file
            .take()
            .ok_or_else(|| TusError::Internal("lock already released".into()))?;
        std::mem::forget(self);
        tokio::task::spawn_blocking(move || {
            file.unlock().map_err(TusError::Io)?;
            let _ = std::fs::remove_file(&path);
            Ok::<(), TusError>(())
        })
        .await
        .map_err(|e| TusError::Internal(format!("unlock join: {e}")))?
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if let Some(f) = self.file.take() {
            let _ = f.unlock();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
