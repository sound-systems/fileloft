use std::io::Cursor;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::Path,
    http::{Request, Response, StatusCode},
    routing::any,
    Router,
};
use fileloft_core::{
    handler::{TusBody, TusHandler, TusRequest, TusResponse},
    lock::SendLocker,
    store::SendDataStore,
};
use http_body_util::BodyExt;
use tokio_util::io::ReaderStream;

/// Mount with [`Router::nest`], e.g. `.nest("/files", tus_router(handler))`.
pub fn tus_router<S, L>(handler: Arc<TusHandler<S, L>>) -> Router
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let h_col = handler.clone();
    let h_res = handler;
    Router::new()
        .route(
            "/",
            any(move |req: Request<Body>| {
                let h = h_col.clone();
                async move { handle_axum(h, req, None).await }
            }),
        )
        .route(
            "/{id}",
            any(move |Path(id): Path<String>, req: Request<Body>| {
                let h = h_res.clone();
                async move { handle_axum(h, req, Some(id)).await }
            }),
        )
}

async fn handle_axum<S, L>(
    handler: Arc<TusHandler<S, L>>,
    req: Request<Body>,
    upload_id: Option<String>,
) -> Response<Body>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let (parts, body) = req.into_parts();
    let method = parts.method;
    let uri = parts.uri;
    let headers = parts.headers;

    let body = if matches!(
        method,
        http::Method::HEAD | http::Method::DELETE | http::Method::OPTIONS | http::Method::GET
    ) {
        None
    } else {
        match body.collect().await {
            Ok(col) => {
                let bytes = col.to_bytes();
                let reader: Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin> =
                    Box::new(Cursor::new(bytes.to_vec()));
                Some(reader)
            }
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("failed to read request body: {e}")))
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
        }
    };

    let tus_req = TusRequest {
        method,
        uri,
        upload_id,
        headers,
        body,
    };
    map_response(handler.handle(tus_req).await)
}

fn map_response(tus: TusResponse) -> Response<Body> {
    let mut res = match tus.body {
        TusBody::Bytes(b) => Response::new(Body::from(b)),
        TusBody::Reader(r) => {
            let stream = ReaderStream::new(r);
            Response::new(Body::from_stream(stream))
        }
    };
    *res.status_mut() = tus.status;
    res.headers_mut().extend(tus.headers);
    res
}
