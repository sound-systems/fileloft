use std::io::Cursor;
use std::sync::Arc;

use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::http::{Method, StatusCode};
use actix_web::web::{self};
use actix_web::{HttpRequest, HttpResponse};
use futures_util::StreamExt;
use tus_core::{
    handler::{TusHandler, TusRequest, TusResponse},
    lock::SendLocker,
    store::SendDataStore,
};

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
    mut payload: web::Payload,
    handler: web::Data<Arc<TusHandler<S, L>>>,
) -> Result<HttpResponse, actix_web::Error>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    handle_actix(handler.get_ref(), &req, &mut payload, None).await
}

async fn dispatch_with_id<S, L>(
    path: web::Path<String>,
    req: HttpRequest,
    mut payload: web::Payload,
    handler: web::Data<Arc<TusHandler<S, L>>>,
) -> Result<HttpResponse, actix_web::Error>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    handle_actix(handler.get_ref(), &req, &mut payload, Some(path.into_inner())).await
}

async fn handle_actix<S, L>(
    handler: &Arc<TusHandler<S, L>>,
    req: &HttpRequest,
    payload: &mut web::Payload,
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
        &Method::HEAD | &Method::DELETE | &Method::OPTIONS
    ) {
        None
    } else {
        let mut buf = Vec::new();
        while let Some(chunk) = payload.next().await {
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);
        }
        let reader: Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin> =
            Box::new(Cursor::new(buf));
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
    res.body(tus.body)
}
