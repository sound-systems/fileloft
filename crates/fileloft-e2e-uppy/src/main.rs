//! Manual E2E server: open the printed URL in a browser and upload via Uppy.

use std::net::Ipv4Addr;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let uploads = PathBuf::from("uploads");
    tokio::fs::create_dir_all(&uploads).await?;

    let (addr, handle) =
        fileloft_e2e_uppy::start_server(uploads.clone(), 3000, Ipv4Addr::UNSPECIFIED.into())
            .await?;

    tracing::info!(
        "Listening on http://{addr} — uploads directory: {}",
        uploads.display()
    );

    handle.await?;
    Ok(())
}
