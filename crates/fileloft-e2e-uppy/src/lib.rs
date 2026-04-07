//! Axum + [`fileloft_axum::tus_router`] + [`fileloft_store_fs::FileStore`] with a vendored Uppy page.
//!
//! Uppy is bundled into `static/vendor/` by `npm run build` (see `package.json`) and embedded or
//! served from the binary so E2E tests do not depend on a CDN.
//!
//! Used by the `fileloft-e2e-uppy` binary and by `tests/e2e.rs`.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use fileloft_axum::tus_router;
use fileloft_core::config::Config;
use fileloft_core::handler::TusHandler;
use fileloft_store_fs::{FileLocker, FileStore};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Embedded shell HTML (loads locally served `/assets/*` Uppy bundle).
pub const INDEX_HTML: &str = include_str!("../static/index.html");

fn static_asset(content_type: &'static str, data: &'static [u8]) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(Bytes::from_static(data)))
        .expect("static asset response")
}

async fn serve_uppy_js() -> impl IntoResponse {
    static_asset(
        "application/javascript; charset=utf-8",
        include_bytes!("../static/vendor/uppy-e2e.js"),
    )
}

async fn serve_uppy_core_css() -> impl IntoResponse {
    static_asset(
        "text/css; charset=utf-8",
        include_bytes!("../static/vendor/uppy-core.min.css"),
    )
}

async fn serve_uppy_dashboard_css() -> impl IntoResponse {
    static_asset(
        "text/css; charset=utf-8",
        include_bytes!("../static/vendor/uppy-dashboard.min.css"),
    )
}

/// Starts the tus server on `0.0.0.0:{port}` (`0` = OS-assigned port).
///
/// Upload data is stored under `data_dir` using [`FileStore`] layout (`{id}`, `{id}.info`, …).
///
/// `bind` is the address to listen on (use [`Ipv4Addr::UNSPECIFIED`] for all interfaces, or
/// loopback for tests).
pub async fn start_server(
    data_dir: PathBuf,
    port: u16,
    bind: IpAddr,
) -> Result<(SocketAddr, JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    tokio::fs::create_dir_all(&data_dir).await?;

    let lock_dir = data_dir.join("locks");
    tokio::fs::create_dir_all(&lock_dir).await?;

    let store = FileStore::new(&data_dir);
    let locker = FileLocker::new(lock_dir);

    let config = Config {
        enable_cors: true,
        base_path: "/files/".to_string(),
        ..Default::default()
    };

    let handler = Arc::new(TusHandler::new(store, Some(locker), config));

    let app = Router::new()
        .route("/", get(|| async move { Html(INDEX_HTML) }))
        .route("/assets/uppy-e2e.js", get(serve_uppy_js))
        .route("/assets/uppy-core.min.css", get(serve_uppy_core_css))
        .route(
            "/assets/uppy-dashboard.min.css",
            get(serve_uppy_dashboard_css),
        )
        .nest("/files", tus_router(handler));

    let listener = TcpListener::bind(SocketAddr::new(bind, port)).await?;
    let addr = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app.into_make_service()).await {
            tracing::error!("server error: {e}");
        }
    });

    Ok((addr, handle))
}
