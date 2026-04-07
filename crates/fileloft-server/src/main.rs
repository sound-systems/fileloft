#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use fileloft_axum::tus_router;
use fileloft_core::config::Config;
use fileloft_core::handler::TusHandler;
#[cfg(any(feature = "backend-s3", feature = "backend-gcs", feature = "backend-azure"))]
use fileloft_core::handler::NoLocker;

// Exactly one backend feature must be enabled.
#[cfg(not(any(
    feature = "backend-fs",
    feature = "backend-s3",
    feature = "backend-gcs",
    feature = "backend-azure",
)))]
compile_error!(
    "No storage backend selected. Enable exactly one of: backend-fs, backend-s3, backend-gcs, backend-azure"
);

#[cfg(any(
    all(feature = "backend-fs", feature = "backend-s3"),
    all(feature = "backend-fs", feature = "backend-gcs"),
    all(feature = "backend-fs", feature = "backend-azure"),
    all(feature = "backend-s3", feature = "backend-gcs"),
    all(feature = "backend-s3", feature = "backend-azure"),
    all(feature = "backend-gcs", feature = "backend-azure"),
))]
compile_error!(
    "Multiple storage backends selected. Enable exactly one of: backend-fs, backend-s3, backend-gcs, backend-azure"
);

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

fn build_config() -> Config {
    let base_path = env_or("FILELOFT_BASE_PATH", "/files/");
    let max_size: u64 = env_opt("FILELOFT_MAX_SIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Config {
        base_path,
        max_size,
        enable_cors: true,
        ..Default::default()
    }
}

fn bind_addr() -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let raw = env_or("FILELOFT_BIND", "0.0.0.0:8080");
    let addr: SocketAddr = raw
        .parse()
        .map_err(|e| format!("invalid FILELOFT_BIND '{raw}': {e}"))?;
    Ok(addr)
}

// ---------------------------------------------------------------------------
// Filesystem backend
// ---------------------------------------------------------------------------
#[cfg(feature = "backend-fs")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use fileloft_store_fs::{FileLocker, FileStore};

    let data_dir = env_or("FILELOFT_DATA_DIR", "/var/lib/fileloft");
    let lock_dir = format!("{data_dir}/locks");

    tokio::fs::create_dir_all(&data_dir).await?;
    tokio::fs::create_dir_all(&lock_dir).await?;

    let store = FileStore::new(&data_dir);
    let locker = FileLocker::new(&lock_dir);
    let handler = Arc::new(TusHandler::new(store, Some(locker), build_config()));

    tracing::info!(backend = "fs", data_dir, "storage configured");
    serve(handler).await
}

// ---------------------------------------------------------------------------
// S3 backend
// ---------------------------------------------------------------------------
#[cfg(feature = "backend-s3")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use fileloft_store_s3::S3Store;

    let bucket = env_opt("FILELOFT_S3_BUCKET")
        .ok_or("FILELOFT_S3_BUCKET is required for the S3 backend")?;

    let mut builder = S3Store::builder(&bucket);

    if let Some(prefix) = env_opt("FILELOFT_S3_PREFIX") {
        builder = builder.prefix(prefix);
    }
    if let Some(endpoint) = env_opt("FILELOFT_S3_ENDPOINT") {
        builder = builder.endpoint_url(endpoint);
    }
    if let Some(region) = env_opt("FILELOFT_S3_REGION") {
        builder = builder.region(region);
    }
    if env_opt("FILELOFT_S3_FORCE_PATH_STYLE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        builder = builder.force_path_style(true);
    }

    let store = builder.build().await?;
    let handler: Arc<TusHandler<S3Store, NoLocker>> =
        Arc::new(TusHandler::new(store, None, build_config()));

    tracing::info!(backend = "s3", bucket, "storage configured");
    serve(handler).await
}

// ---------------------------------------------------------------------------
// GCS backend
// ---------------------------------------------------------------------------
#[cfg(feature = "backend-gcs")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use fileloft_store_gcs::GcsStore;

    let bucket = env_opt("FILELOFT_GCS_BUCKET")
        .ok_or("FILELOFT_GCS_BUCKET is required for the GCS backend")?;

    let mut builder = GcsStore::builder(&bucket);

    if let Some(prefix) = env_opt("FILELOFT_GCS_PREFIX") {
        builder = builder.prefix(prefix);
    }

    let store = builder.build().await?;
    let handler: Arc<TusHandler<GcsStore, NoLocker>> =
        Arc::new(TusHandler::new(store, None, build_config()));

    tracing::info!(backend = "gcs", bucket, "storage configured");
    serve(handler).await
}

// ---------------------------------------------------------------------------
// Azure Blob Storage backend
// ---------------------------------------------------------------------------
#[cfg(feature = "backend-azure")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use fileloft_store_azure::AzureStore;

    let container = env_opt("FILELOFT_AZURE_CONTAINER")
        .ok_or("FILELOFT_AZURE_CONTAINER is required for the Azure backend")?;

    let mut builder = AzureStore::builder(&container);

    if let Some(prefix) = env_opt("FILELOFT_AZURE_PREFIX") {
        builder = builder.prefix(prefix);
    }
    if let Some(conn) = env_opt("FILELOFT_AZURE_CONNECTION_STRING") {
        builder = builder.connection_string(conn);
    }
    if let Some(account) = env_opt("FILELOFT_AZURE_ACCOUNT") {
        builder = builder.account(account);
    }

    let store = builder.build().await?;
    let handler: Arc<TusHandler<AzureStore, NoLocker>> =
        Arc::new(TusHandler::new(store, None, build_config()));

    tracing::info!(backend = "azure", container, "storage configured");
    serve(handler).await
}

// ---------------------------------------------------------------------------
// Shared server bootstrap
// ---------------------------------------------------------------------------
async fn serve<S, L>(handler: Arc<TusHandler<S, L>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: fileloft_core::SendDataStore + Send + Sync + 'static,
    L: fileloft_core::SendLocker + Send + Sync + 'static,
{
    let config = build_config();
    let base_path = config.base_path.trim_end_matches('/');
    let app = axum::Router::new().nest(base_path, tus_router(handler));

    let addr = bind_addr()?;
    tracing::info!(%addr, path = base_path, "server listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}
