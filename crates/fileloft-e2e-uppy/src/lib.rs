//! Axum + [`fileloft_axum::tus_router`] + [`fileloft_store_fs::FileStore`] with an embedded Uppy page.
//!
//! Used by the `fileloft-e2e-uppy` binary and by `tests/e2e.rs`.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fileloft_axum::tus_router;
use fileloft_core::config::Config;
use fileloft_core::handler::TusHandler;
use fileloft_store_fs::{FileLocker, FileStore};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Embedded Uppy Dashboard page (ESM from CDN).
pub const INDEX_HTML: &str = include_str!("../static/index.html");

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
