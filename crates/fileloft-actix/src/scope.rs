use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::http::{Method, StatusCode};
use actix_web::web::{self};
use actix_web::{HttpRequest, HttpResponse};
use bytes::{Buf, Bytes};
use fileloft_core::{
    handler::{TusBody, TusHandler, TusRequest, TusResponse},
    lock::SendLocker,
    store::SendDataStore,
};
use futures_util::StreamExt;
use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;

/// Register with `App::new().app_data(handler).service(tus_scope::<S,L>())`.
pub fn tus_scope<S, L>() -> actix_web::Scope
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    web::scope("")
        .route("", web::route().to(dispatch::<S, L>))
        .route("/{id}", web::route().to(dispatch_with_id::<S, L>))
}

async fn dispatch<S, L>(
    req: HttpRequest,
    payload: web::Payload,
    handler: web::Data<Arc<TusHandler<S, L>>>,
) -> Result<HttpResponse, actix_web::Error>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    handle_actix(handler.get_ref(), &req, payload, None).await
}

async fn dispatch_with_id<S, L>(
    path: web::Path<String>,
    req: HttpRequest,
    payload: web::Payload,
    handler: web::Data<Arc<TusHandler<S, L>>>,
) -> Result<HttpResponse, actix_web::Error>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    handle_actix(handler.get_ref(), &req, payload, Some(path.into_inner())).await
}

async fn handle_actix<S, L>(
    handler: &Arc<TusHandler<S, L>>,
    req: &HttpRequest,
    payload: web::Payload,
    upload_id: Option<String>,
) -> Result<HttpResponse, actix_web::Error>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let method = actix_to_http_method(req.method());
    let uri = actix_to_http_uri(req);
    let headers = headers_to_http(req);

    let body = if matches!(
        req.method(),
        &Method::HEAD | &Method::DELETE | &Method::OPTIONS | &Method::GET
    ) {
        None
    } else {
        let (tx, rx) = mpsc::channel(8);
        actix_web::rt::spawn(async move {
            let mut payload = payload;
            while let Some(chunk) = payload.next().await {
                let item = chunk.map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("failed to read request body: {e}"),
                    )
                });
                if tx.send(item).await.is_err() {
                    break;
                }
            }
        });
        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(ChannelReader::new(rx));
        Some(reader)
    };

    let tus_req = TusRequest {
        method,
        uri,
        upload_id,
        headers,
        body,
    };
    let tus = handler.handle(tus_req).await;
    Ok(map_response(tus))
}

struct ChannelReader {
    rx: mpsc::Receiver<Result<Bytes, io::Error>>,
    current: Option<Bytes>,
}

impl ChannelReader {
    fn new(rx: mpsc::Receiver<Result<Bytes, io::Error>>) -> Self {
        Self { rx, current: None }
    }
}

impl AsyncRead for ChannelReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if let Some(current) = &mut self.current {
                let n = current.len().min(buf.remaining());
                if n == 0 {
                    return Poll::Ready(Ok(()));
                }
                buf.put_slice(&current[..n]);
                current.advance(n);
                if current.is_empty() {
                    self.current = None;
                }
                return Poll::Ready(Ok(()));
            }

            match Pin::new(&mut self.rx).poll_recv(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    if !bytes.is_empty() {
                        self.current = Some(bytes);
                    }
                }
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(err)),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn actix_to_http_method(m: &Method) -> http::Method {
    http::Method::from_bytes(m.as_str().as_bytes()).unwrap_or(http::Method::GET)
}

fn actix_to_http_uri(req: &HttpRequest) -> http::Uri {
    req.uri()
        .to_string()
        .parse()
        .unwrap_or_else(|_| http::Uri::from_static("/"))
}

fn headers_to_http(req: &HttpRequest) -> http::HeaderMap {
    let mut out = http::HeaderMap::new();
    for (name, value) in req.headers().iter() {
        if let (Ok(n), Ok(v)) = (
            http::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            http::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out.append(n, v);
        }
    }
    out
}

fn map_response(tus: TusResponse) -> HttpResponse {
    let status =
        StatusCode::from_u16(tus.status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut res = HttpResponse::build(status);
    for (k, v) in tus.headers.iter() {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_str().as_bytes()),
            HeaderValue::from_bytes(v.as_bytes()),
        ) {
            res.insert_header((name, val));
        }
    }
    match tus.body {
        TusBody::Bytes(b) => res.body(b),
        TusBody::Reader(r) => {
            let stream = ReaderStream::new(r).map(|item| {
                item.map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))
            });
            res.streaming(stream)
        }
    }
}
