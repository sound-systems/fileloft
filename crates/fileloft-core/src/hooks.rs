use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::error::TusError;
use crate::info::{UploadId, UploadInfo, UploadInfoChanges};

/// All lifecycle events the handler can emit.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// A new upload slot was successfully created (POST).
    UploadCreated { info: UploadInfo },
    /// An upload reached 100% — offset == size.
    UploadFinished { info: UploadInfo },
    /// An upload was explicitly terminated (DELETE).
    UploadTerminated { id: UploadId },
    /// A chunk was written; emitted after each successful PATCH.
    UploadProgress { info: UploadInfo },
}

/// Boxed async future returned by hook callbacks.
pub type HookFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Pre-create callback. Receives proposed `UploadInfo`; may return modified fields
/// (e.g. override ID or metadata) or reject creation with an `Err`.
pub type PreCreateCallback =
    Arc<dyn Fn(UploadInfo) -> HookFuture<Result<UploadInfoChanges, TusError>> + Send + Sync>;

/// Pre-finish callback. Called after all bytes are written but before 204 is sent.
/// Return `Err` to abort and respond with an error.
pub type PreFinishCallback =
    Arc<dyn Fn(UploadInfo) -> HookFuture<Result<(), TusError>> + Send + Sync>;

/// Pre-terminate callback. Called before DELETE is processed.
/// Return `Err` to reject the termination.
pub type PreTerminateCallback =
    Arc<dyn Fn(UploadInfo) -> HookFuture<Result<(), TusError>> + Send + Sync>;

/// Sender side of the lifecycle event broadcast channel.
/// Callers call `.subscribe()` to receive a `Receiver<HookEvent>`.
pub type HookSender = broadcast::Sender<HookEvent>;

/// Hook configuration attached to `Config`.
#[derive(Clone, Default)]
pub struct HookConfig {
    /// Capacity of the broadcast channel. `0` means hooks are disabled.
    pub channel_capacity: usize,
    pub pre_create: Option<PreCreateCallback>,
    pub pre_finish: Option<PreFinishCallback>,
    pub pre_terminate: Option<PreTerminateCallback>,
}

impl std::fmt::Debug for HookConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookConfig")
            .field("channel_capacity", &self.channel_capacity)
            .field("pre_create", &self.pre_create.is_some())
            .field("pre_finish", &self.pre_finish.is_some())
            .field("pre_terminate", &self.pre_terminate.is_some())
            .finish()
    }
}

impl HookConfig {
    /// Returns true if any hooks are configured.
    pub fn has_hooks(&self) -> bool {
        self.channel_capacity > 0
            || self.pre_create.is_some()
            || self.pre_finish.is_some()
            || self.pre_terminate.is_some()
    }
}
