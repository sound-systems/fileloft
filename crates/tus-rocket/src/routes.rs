use std::io::Cursor;
use std::sync::Arc;

use rocket::data::{Data, ToByteUnit};
use rocket::http::{Header, Method, Status};
use rocket::response::Response;
use rocket::route::{Handler, Outcome, Route};
use rocket::tokio::io::AsyncReadExt;
use rocket::Request;
use tus_core::{
    handler::{TusHandler, TusRequest, TusResponse},
    lock::SendLocker,
    store::SendDataStore,
};

/// Mount with `rocket.mount("/files", tus_routes(handler))`.
pub fn tus_routes<S, L>(handler: Arc<TusHandler<S, L>>) -> Vec<Route>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let inner = TusRocketHandler { handler };
    let methods = [
        Method::Options,
        Method::Head,
        Method::Post,
        Method::Patch,
        Method::Delete,
    ];
    let mut routes = Vec::new();
    for m in methods {
        routes.push(Route::new(m, "/", inner.clone()));
        routes.push(Route::new(m, "/<id>", inner.clone()));
    }
    routes
}

struct TusRocketHandler<S, L> {
    handler: Arc<TusHandler<S, L>>,
}

impl<S, L> Clone for TusRocketHandler<S, L> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
        }
    }
}

#[rocket::async_trait]
impl<S, L> Handler for TusRocketHandler<S, L>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    async fn handle<'r>(&self, req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r> {
        match handle_inner(&self.handler, req, data).await {
            Ok(resp) => Outcome::Success(resp),
            Err(status) => Outcome::Error(status),
        }
    }
}

async fn handle_inner<'r, S, L>(
    handler: &Arc<TusHandler<S, L>>,
    req: &'r Request<'_>,
    data: Data<'r>,
) -> Result<Response<'r>, Status>
where
    S: SendDataStore + Send + Sync + 'static,
    L: SendLocker + Send + Sync + 'static,
{
    let method = rocket_to_http_method(&req.method());
    let uri = rocket_to_http_uri(req);
    let headers = rocket_headers_to_http(req);

    let body = if matches!(
        req.method(),
        Method::Head | Method::Delete | Method::Options
    ) {
        None
    } else {
        let mut stream = data.open(512.mebibytes());
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|_| Status::InternalServerError)?;
        let reader: Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin> =
            Box::new(Cursor::new(buf));
        Some(reader)
    };

    let upload_id = rocket_upload_id(req);

    let tus_req = TusRequest {
        method,
        uri,
        upload_id,
        headers,
        body,
    };
    let tus = handler.handle(tus_req).await;
    rocket_response(tus)
}

fn rocket_to_http_method(m: &Method) -> http::Method {
    http::Method::from_bytes(m.as_str().as_bytes()).unwrap_or(http::Method::GET)
}

fn rocket_to_http_uri(req: &Request<'_>) -> http::Uri {
    req.uri()
        .to_string()
        .parse()
        .unwrap_or_else(|_| http::Uri::from_static("/"))
}

fn rocket_headers_to_http(req: &Request<'_>) -> http::HeaderMap {
    let mut out = http::HeaderMap::new();
    for header in req.headers().iter() {
        let name = header.name.as_str();
        let value = header.value.as_bytes();
        if let (Ok(n), Ok(v)) = (
            http::header::HeaderName::from_bytes(name.as_bytes()),
            http::header::HeaderValue::from_bytes(value),
        ) {
            out.append(n, v);
        }
    }
    out
}

fn rocket_upload_id(req: &Request<'_>) -> Option<String> {
    let path = req.uri().path().as_str();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [_, id] => Some((*id).to_string()),
        _ => None,
    }
}

fn rocket_response<'r>(tus: TusResponse) -> Result<Response<'r>, Status> {
    let status = Status::from_code(tus.status.as_u16()).unwrap_or(Status::InternalServerError);
    let mut builder = Response::build();
    builder.status(status);
    let pairs: Vec<_> = tus
        .headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.as_bytes().to_vec()))
        .collect();
    for (name, val) in pairs {
        let value = String::from_utf8_lossy(&val).into_owned();
        builder.header(Header::new(name, value));
    }
    let body = tus.body.to_vec();
    builder.sized_body(body.len(), Cursor::new(body));
    Ok(builder.finalize())
}
