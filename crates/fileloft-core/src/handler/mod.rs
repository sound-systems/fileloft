mod delete;
mod get;
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
    proto::{
        HDR_ACCESS_CONTROL_ALLOW_CREDENTIALS, HDR_CACHE_CONTROL, HDR_TUS_RESUMABLE, TUS_VERSION,
    },
    store::SendDataStore,
    util::static_header,
};

/// Response body: small protocol messages or a streamed download.
pub enum TusBody {
    Bytes(Bytes),
    /// Streamed body (e.g. GET download). Framework layers map this to a streaming HTTP body.
    Reader(Box<dyn tokio::io::AsyncRead + Send + Unpin>),
}

impl std::fmt::Debug for TusBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(b) => f.debug_tuple("Bytes").field(b).finish(),
            Self::Reader(_) => f.write_str("Reader(..)"),
        }
    }
}

/// Incoming request as seen by the tus handler.
/// Framework integrations construct this from their native request type.
pub struct TusRequest {
    pub method: Method,
    pub uri: Uri,
    /// Upload ID extracted from the URL path by the framework router.
    /// Present for HEAD / PATCH / DELETE / GET; absent for OPTIONS and POST.
    pub upload_id: Option<String>,
    pub headers: HeaderMap,
    /// Streaming body. `None` for HEAD / DELETE / OPTIONS / GET.
    pub body: Option<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>>,
}

/// Outgoing response produced by the tus handler.
/// Framework integrations convert this into their native response type.
pub struct TusResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: TusBody,
}

impl TusResponse {
    /// When the body is [`TusBody::Bytes`], returns its slice (for tests and small responses).
    pub fn bytes_slice(&self) -> Option<&[u8]> {
        match &self.body {
            TusBody::Bytes(b) => Some(b.as_ref()),
            TusBody::Reader(_) => None,
        }
    }
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
            Method::GET => get::handle(self, &req).await,
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
        self.response_with_body(status, extra_headers, TusBody::Bytes(body))
    }

    pub(crate) fn response_with_body(
        &self,
        status: StatusCode,
        extra_headers: HeaderMap,
        body: TusBody,
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
            body: TusBody::Bytes(body),
        }
    }

    /// Headers added to every response.
    fn base_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(HDR_TUS_RESUMABLE, static_header(TUS_VERSION));
        h.insert(HDR_CACHE_CONTROL, static_header("no-store"));
        let cors = &self.config.cors;
        if cors.enabled {
            if let Ok(v) = HeaderValue::from_str(&cors.allow_origin) {
                h.insert(crate::proto::HDR_ACCESS_CONTROL_ALLOW_ORIGIN, v);
            }
            if cors.allow_credentials {
                h.insert(HDR_ACCESS_CONTROL_ALLOW_CREDENTIALS, static_header("true"));
            }
            let mut expose = String::from(
                "Upload-Offset,Upload-Length,Upload-Metadata,Upload-Expires,\
                 Upload-Defer-Length,Location,Tus-Resumable,Tus-Version,Tus-Extension,\
                 Tus-Max-Size,Tus-Checksum-Algorithm,Content-Length,Content-Type",
            );
            for extra in &cors.extra_expose_headers {
                expose.push(',');
                expose.push_str(extra.trim());
            }
            if let Ok(v) = expose.parse() {
                h.insert(crate::proto::HDR_ACCESS_CONTROL_EXPOSE_HEADERS, v);
            }
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
