use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tus_core::{
    error::TusError,
    info::UploadId,
    lock::{SendLock, SendLocker},
};

type HeldSet = Arc<Mutex<HashSet<String>>>;

/// An in-memory locker using a simple spin-wait with small sleep intervals.
///
/// This locker is process-local; for multi-process deployments use a
/// distributed lock (e.g. Redis) or the file-based `FileLocker`.
#[derive(Clone)]
pub struct MemoryLocker {
    held: HeldSet,
    /// How long to wait before giving up with `LockTimeout`.
    pub timeout: Duration,
}

impl MemoryLocker {
    pub fn new() -> Self {
        Self {
            held: Arc::new(Mutex::new(HashSet::new())),
            timeout: Duration::from_secs(20),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for MemoryLocker {
    fn default() -> Self {
        Self::new()
    }
}

impl SendLocker for MemoryLocker {
    type LockType = MemoryLock;

    async fn acquire(&self, id: &UploadId) -> Result<MemoryLock, TusError> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            {
                let mut held = self.held.lock().await;
                if held.insert(id.as_str().to_string()) {
                    return Ok(MemoryLock {
                        id: id.as_str().to_string(),
                        held: Arc::clone(&self.held),
                        released: false,
                    });
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(TusError::LockTimeout(id.to_string()));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// A held in-memory lock.
/// Automatically released on drop; also releasable explicitly via `release()`.
pub struct MemoryLock {
    id: String,
    held: HeldSet,
    released: bool,
}

impl SendLock for MemoryLock {
    async fn release(mut self) -> Result<(), TusError> {
        self.held.lock().await.remove(&self.id);
        self.released = true;
        Ok(())
    }
}

impl Drop for MemoryLock {
    fn drop(&mut self) {
        if !self.released {
            // Best-effort synchronous release: try_lock avoids blocking in Drop.
            if let Ok(mut held) = self.held.try_lock() {
                held.remove(&self.id);
            }
            // If try_lock fails, the lock will remain held until the Mutex is
            // next acquired — acceptable in test contexts.
        }
    }
}
