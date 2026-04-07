mod delete;
mod head;
mod options;
mod patch;
mod post;

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use tokio::sync::broadcast;

use crate::{
    config::Config,
    error::TusError,
    hooks::{HookEvent, HookSender},
    lock::SendLocker,
    proto::{HDR_CACHE_CONTROL, HDR_TUS_RESUMABLE, TUS_VERSION},
    store::SendDataStore,
    util::static_header,
};

/// Incoming request as seen by the tus handler.
/// Framework integrations construct this from their native request type.
pub struct TusRequest {
    pub method: Method,
    pub uri: Uri,
    /// Upload ID extracted from the URL path by the framework router.
    /// Present for HEAD / PATCH / DELETE; absent for OPTIONS and POST.
    pub upload_id: Option<String>,
    pub headers: HeaderMap,
    /// Streaming body. `None` for HEAD / DELETE / OPTIONS.
    pub body: Option<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>>,
}

/// Outgoing response produced by the tus handler.
/// Framework integrations convert this into their native response type.
pub struct TusResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    /// Body bytes — always small for tus (empty or short error text).
    pub body: Bytes,
}

/// The central tus protocol handler.
///
/// Wrap in `Arc<TusHandler<S, L>>` and share across request-handling tasks.
///
/// # Type Parameters
/// - `S`: Storage backend implementing [`SendDataStore`].
/// - `L`: Optional locker implementing [`SendLocker`]. Use `NoLocker` if concurrency
///   control is handled by the store itself or is not needed.
pub struct TusHandler<S, L = NoLocker> {
    pub(crate) store: S,
    pub(crate) locker: Option<L>,
    pub(crate) config: Arc<Config>,
    pub(crate) hook_tx: Option<HookSender>,
}

impl<S, L> TusHandler<S, L>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    pub fn new(store: S, locker: Option<L>, config: Config) -> Self {
        let hook_tx = if config.hooks.channel_capacity > 0 {
            let (tx, _) = broadcast::channel(config.hooks.channel_capacity);
            Some(tx)
        } else {
            None
        };
        Self {
            store,
            locker,
            config: Arc::new(config),
            hook_tx,
        }
    }

    /// Subscribe to lifecycle events. Returns `None` if hooks are not configured.
    pub fn hook_receiver(&self) -> Option<broadcast::Receiver<HookEvent>> {
        self.hook_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// Main dispatch — routes to the appropriate sub-handler.
    pub async fn handle(&self, req: TusRequest) -> TusResponse {
        let result = match req.method {
            Method::OPTIONS => options::handle(self, &req).await,
            Method::HEAD => head::handle(self, &req).await,
            Method::POST => post::handle(self, req).await,
            Method::PATCH => patch::handle(self, req).await,
            Method::DELETE => delete::handle(self, &req).await,
            _ => Err(TusError::MethodNotAllowed),
        };
        match result {
            Ok(resp) => resp,
            Err(err) => self.error_response(err),
        }
    }

    /// Build a response with common tus headers added.
    pub(crate) fn response(
        &self,
        status: StatusCode,
        extra_headers: HeaderMap,
        body: Bytes,
    ) -> TusResponse {
        let mut headers = self.base_headers();
        headers.extend(extra_headers);
        TusResponse {
            status,
            headers,
            body,
        }
    }

    /// Build an error response from a `TusError`.
    pub(crate) fn error_response(&self, err: TusError) -> TusResponse {
        let status = err.status_code();
        let body = Bytes::from(err.to_string());
        let mut headers = self.base_headers();
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        TusResponse {
            status,
            headers,
            body,
        }
    }

    /// Headers added to every response.
    fn base_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(HDR_TUS_RESUMABLE, static_header(TUS_VERSION));
        h.insert(HDR_CACHE_CONTROL, static_header("no-store"));
        if self.config.enable_cors {
            h.insert(
                crate::proto::HDR_ACCESS_CONTROL_ALLOW_ORIGIN,
                static_header("*"),
            );
            h.insert(
                crate::proto::HDR_ACCESS_CONTROL_EXPOSE_HEADERS,
                static_header(
                    "Upload-Offset,Upload-Length,Upload-Metadata,Upload-Expires,\
                     Upload-Defer-Length,Location,Tus-Resumable,Tus-Version,Tus-Extension,\
                     Tus-Max-Size,Tus-Checksum-Algorithm",
                ),
            );
        }
        h
    }

    /// Emit a hook event (non-blocking; missed if no subscriber).
    pub(crate) fn emit(&self, event: HookEvent) {
        if let Some(tx) = &self.hook_tx {
            let _ = tx.send(event);
        }
    }
}

/// A no-op locker used when the caller passes `None` for the locker type.
pub struct NoLocker;

impl crate::lock::SendLock for NoLocker {
    async fn release(self) -> Result<(), TusError> {
        Ok(())
    }
}

impl crate::lock::SendLocker for NoLocker {
    type LockType = NoLocker;
    async fn acquire(&self, _id: &crate::info::UploadId) -> Result<NoLocker, TusError> {
        Ok(NoLocker)
    }
}
