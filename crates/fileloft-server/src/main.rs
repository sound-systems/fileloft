#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use fileloft_axum::tus_router;
use fileloft_core::config::{Config, CorsConfig, Extensions};
#[cfg(any(
    feature = "backend-s3",
    feature = "backend-gcs",
    feature = "backend-azure"
))]
use fileloft_core::handler::NoLocker;
use fileloft_core::handler::TusHandler;

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

fn env_truthy(name: &str) -> bool {
    env_opt(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn parse_comma_headers(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn build_config() -> Result<Config, Box<dyn std::error::Error + Send + Sync>> {
    let base_path = env_or("FILELOFT_BASE_PATH", "/files/");
    let max_size: u64 = env_opt("FILELOFT_MAX_SIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut extensions = Extensions::default();

    if env_truthy("FILELOFT_DISABLE_TERMINATION") {
        extensions.termination = false;
    }
    if env_truthy("FILELOFT_ENABLE_CONCATENATION") {
        extensions.concatenation = true;
    }
    if env_truthy("FILELOFT_ENABLE_CLEANUP_CONCAT_PARTIALS") {
        extensions.cleanup_concat_partials = true;
    }
    if env_truthy("FILELOFT_ENABLE_EXPIRATION") {
        extensions.expiration = true;
    }
    if let Some(s) = env_opt("FILELOFT_EXPIRATION_TTL") {
        let secs: u64 = s
            .parse()
            .map_err(|_| "invalid FILELOFT_EXPIRATION_TTL (expect seconds as u64)")?;
        extensions.expiration_ttl = Some(Duration::from_secs(secs));
    }
    if env_truthy("FILELOFT_DISABLE_CHECKSUM") {
        extensions.checksum = false;
    }
    if env_truthy("FILELOFT_DISABLE_CREATION") {
        extensions.creation = false;
    }
    if env_truthy("FILELOFT_DISABLE_CREATION_WITH_UPLOAD") {
        extensions.creation_with_upload = false;
    }
    if env_truthy("FILELOFT_DISABLE_DEFER_LENGTH") {
        extensions.creation_defer_length = false;
    }

    let lock_timeout_secs: u64 = env_opt("FILELOFT_LOCK_TIMEOUT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let cors = CorsConfig {
        enabled: !env_truthy("FILELOFT_DISABLE_CORS"),
        allow_origin: env_or("FILELOFT_CORS_ALLOW_ORIGIN", "*"),
        allow_credentials: env_truthy("FILELOFT_CORS_ALLOW_CREDENTIALS"),
        extra_allow_headers: parse_comma_headers(env_opt("FILELOFT_CORS_ALLOW_HEADERS").as_deref()),
        extra_expose_headers: parse_comma_headers(
            env_opt("FILELOFT_CORS_EXPOSE_HEADERS").as_deref(),
        ),
        max_age: env_opt("FILELOFT_CORS_MAX_AGE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(86400),
    };

    if env_truthy("FILELOFT_ENABLE_H2C") {
        tracing::info!(
            "FILELOFT_ENABLE_H2C: HTTP/2 cleartext is available; axum is built with the http2 feature"
        );
    }

    Ok(Config {
        base_path,
        base_url: env_opt("FILELOFT_BASE_URL"),
        max_size,
        extensions,
        lock_timeout: Duration::from_secs(lock_timeout_secs),
        cors,
        trust_forwarded_headers: env_truthy("FILELOFT_BEHIND_PROXY"),
        hooks: Default::default(),
        enable_download: env_truthy("FILELOFT_ENABLE_DOWNLOAD"),
    })
}

fn bind_addr() -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let raw = env_or("FILELOFT_BIND", "0.0.0.0:8080");
    let addr: SocketAddr = raw
        .parse()
        .map_err(|e| format!("invalid FILELOFT_BIND '{raw}': {e}"))?;
    Ok(addr)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let sigterm = async {
        let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("register SIGTERM handler");
        sig.recv().await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT (Ctrl+C), shutting down"),
        _ = sigterm => tracing::info!("received SIGTERM, shutting down"),
    }
}

fn build_router<S, L>(handler: Arc<TusHandler<S, L>>, base_path: &str) -> Router
where
    S: fileloft_core::SendDataStore + Send + Sync + 'static,
    L: fileloft_core::SendLocker + Send + Sync + 'static,
{
    Router::new().nest(base_path, tus_router(handler))
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
    let config = build_config()?;
    let handler = Arc::new(TusHandler::new(store, Some(locker), config.clone()));

    tracing::info!(backend = "fs", data_dir, "storage configured");
    serve(handler, config).await
}

// ---------------------------------------------------------------------------
// S3 backend
// ---------------------------------------------------------------------------
#[cfg(feature = "backend-s3")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use fileloft_store_s3::S3Store;

    let bucket =
        env_opt("FILELOFT_S3_BUCKET").ok_or("FILELOFT_S3_BUCKET is required for the S3 backend")?;

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
    let config = build_config()?;
    let handler: Arc<TusHandler<S3Store, NoLocker>> =
        Arc::new(TusHandler::new(store, None, config.clone()));

    tracing::info!(backend = "s3", bucket, "storage configured");
    serve(handler, config).await
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
    let config = build_config()?;
    let handler: Arc<TusHandler<GcsStore, NoLocker>> =
        Arc::new(TusHandler::new(store, None, config.clone()));

    tracing::info!(backend = "gcs", bucket, "storage configured");
    serve(handler, config).await
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
    let config = build_config()?;
    let handler: Arc<TusHandler<AzureStore, NoLocker>> =
        Arc::new(TusHandler::new(store, None, config.clone()));

    tracing::info!(backend = "azure", container, "storage configured");
    serve(handler, config).await
}

// ---------------------------------------------------------------------------
// Shared server bootstrap
// ---------------------------------------------------------------------------
async fn serve<S, L>(
    handler: Arc<TusHandler<S, L>>,
    config: Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: fileloft_core::SendDataStore + Send + Sync + 'static,
    L: fileloft_core::SendLocker + Send + Sync + 'static,
{
    let base_path = config.base_path.trim_end_matches('/');
    let app = build_router(handler, base_path);

    #[cfg(not(unix))]
    if env_opt("FILELOFT_UNIX_SOCKET").is_some() {
        return Err("FILELOFT_UNIX_SOCKET is only supported on Unix".into());
    }

    if env_opt("FILELOFT_UNIX_SOCKET").is_some()
        && (env_opt("FILELOFT_TLS_CERT").is_some() || env_opt("FILELOFT_TLS_KEY").is_some())
    {
        return Err(
            "FILELOFT_UNIX_SOCKET cannot be used together with FILELOFT_TLS_CERT / FILELOFT_TLS_KEY"
                .into(),
        );
    }

    #[cfg(unix)]
    if let Some(sock_path) = env_opt("FILELOFT_UNIX_SOCKET") {
        return serve_unix(sock_path, app).await;
    }

    if let (Some(cert), Some(key)) = (env_opt("FILELOFT_TLS_CERT"), env_opt("FILELOFT_TLS_KEY")) {
        let addr = bind_addr()?;
        tracing::info!(%addr, path = base_path, "server listening (TLS)");
        return serve_tls(addr, cert, key, app).await;
    }

    let addr = bind_addr()?;
    tracing::info!(%addr, path = base_path, "server listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[cfg(unix)]
async fn serve_unix(
    path: String,
    app: Router,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::net::UnixListener;

    let _ = tokio::fs::remove_file(&path).await;
    let uds = UnixListener::bind(&path).map_err(|e| format!("bind unix socket {path}: {e}"))?;
    tracing::info!(%path, "server listening on unix socket");
    axum::serve(uds, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn serve_tls(
    addr: SocketAddr,
    cert: String,
    key: String,
    app: Router,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use axum_server::tls_rustls::RustlsConfig;
    use axum_server::{bind_rustls, Handle};

    let rustls_config = RustlsConfig::from_pem_file(cert, key)
        .await
        .map_err(|e| format!("TLS: {e}"))?;

    let _mode = env_or("FILELOFT_TLS_MODE", "tls12");
    if _mode != "tls12" {
        tracing::warn!(
            mode = %_mode,
            "FILELOFT_TLS_MODE is reserved for future cipher/protocol tuning; using Rustls defaults"
        );
    }

    let shutdown_secs: u64 = env_opt("FILELOFT_SHUTDOWN_TIMEOUT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let handle = Handle::new();
    let h2 = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        h2.graceful_shutdown(Some(Duration::from_secs(shutdown_secs)));
    });

    bind_rustls(addr, rustls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .map_err(|e| format!("server: {e}"))?;
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
