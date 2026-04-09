<p align="center">
  <img src="docs-site/static/logo.png" alt="fileloft" width="240" />
</p>

# fileloft

**fileloft** is a Rust implementation of the [tus](https://tus.io) resumable upload protocol. It ships as a small set of composable crates you can embed in an existing HTTP server, or run as a standalone tus endpoint via the published container image.

- **Framework agnostic** — protocol core with no transport assumptions, plus adapters for Axum, Actix Web, and Rocket.
- **Pluggable storage** — a `DataStore` trait with filesystem, S3, GCS, and Azure Blob Storage backends.
- **Standalone or embedded** — use it as a library or run the prebuilt image when you only need a tus endpoint.
- **Safe by default** — `#![forbid(unsafe_code)]` across the workspace, with conservative defaults for limits, locking, and checksums.

Crates: `fileloft-core`, `fileloft-store-fs`, `fileloft-store-s3`, `fileloft-store-gcs`, `fileloft-store-azure`, `fileloft-axum`, `fileloft-actix`, `fileloft-rocket`.

## Getting started

### Use as a library (Axum)

Add dependencies (versions should match what you use elsewhere; see [crates.io](https://crates.io/crates/fileloft-core)):

```toml
[dependencies]
fileloft-core         = "0.1"
fileloft-store-fs     = "0.1"
fileloft-axum         = "0.1"   # or fileloft-actix / fileloft-rocket
tokio                 = { version = "1", features = ["full"] }
```

Mount a tus router (other adapters follow the same pattern: build a `TusHandler`, then use the framework-specific router):

```rust
use std::sync::Arc;

use fileloft_axum::tus_router;
use fileloft_core::{Config, TusHandler};
use fileloft_store_fs::{FileLocker, FileStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = "/var/lib/fileloft";
    let store = FileStore::new(root);
    let locker = FileLocker::new(format!("{root}/locks"));
    let handler = Arc::new(TusHandler::new(store, Some(locker), Config::default()));

    let app = axum::Router::new().nest("/files", tus_router(handler));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

Full tus 1.0.0 core plus optional extensions (`creation`, `expiration`, `checksum`, `termination`, `concatenation`) are configured via `Config`. API details: [docs.rs/fileloft-core](https://docs.rs/fileloft-core).

### Run the container

A separate image variant is published for each storage backend. The default
(`:latest`) uses the local filesystem.

| Tag | Backend | Example |
| --- | --- | --- |
| `latest`, `fs` | Filesystem | `ghcr.io/sound-systems/fileloft:latest` |
| `s3` | Amazon S3 / S3-compatible | `ghcr.io/sound-systems/fileloft:s3` |
| `gcs` | Google Cloud Storage | `ghcr.io/sound-systems/fileloft:gcs` |
| `azure` | Azure Blob Storage | `ghcr.io/sound-systems/fileloft:azure` |

**Filesystem (default):**

```bash
docker run --rm \
  -p 8080:8080 \
  -v fileloft-data:/var/lib/fileloft \
  ghcr.io/sound-systems/fileloft:latest
```

**S3:**

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_S3_BUCKET=my-uploads \
  -e AWS_ACCESS_KEY_ID \
  -e AWS_SECRET_ACCESS_KEY \
  -e AWS_REGION=us-east-1 \
  ghcr.io/sound-systems/fileloft:s3
```

**GCS:**

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_GCS_BUCKET=my-uploads \
  -v /path/to/keyfile.json:/credentials.json:ro \
  -e GOOGLE_APPLICATION_CREDENTIALS=/credentials.json \
  ghcr.io/sound-systems/fileloft:gcs
```

**Azure:**

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_AZURE_CONTAINER=my-uploads \
  -e AZURE_STORAGE_CONNECTION_STRING \
  ghcr.io/sound-systems/fileloft:azure
```

All variants share these common environment variables (see also [tusd-style configuration](https://tus.github.io/tusd/getting-started/configuration/)):

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_BIND` | `0.0.0.0:8080` | TCP address when not using `FILELOFT_UNIX_SOCKET` or TLS-only paths. |
| `FILELOFT_UNIX_SOCKET` | _unset_ | Unix domain socket path (Unix only; mutually exclusive with TLS bind below). |
| `FILELOFT_MAX_SIZE` | _unset_ | Maximum allowed upload size, in bytes (`0` = unlimited). |
| `FILELOFT_BASE_PATH` | `/files/` | Path the tus endpoints are mounted under. |
| `FILELOFT_BASE_URL` | _unset_ | Absolute base URL for `Location` when it cannot be inferred from the request. |
| `FILELOFT_BEHIND_PROXY` | `false` | Trust `X-Forwarded-*` when building URLs (set when behind a reverse proxy). |
| `FILELOFT_LOCK_TIMEOUT` | `20` | Lock wait timeout (seconds) before `408`. |
| `FILELOFT_SHUTDOWN_TIMEOUT` | `10` | Graceful shutdown window for the **TLS** server path (`axum-server`); plain TCP uses Axum’s default graceful stop. |
| `FILELOFT_DISABLE_CORS` | `false` | Disable CORS headers entirely. |
| `FILELOFT_CORS_ALLOW_ORIGIN` | `*` | `Access-Control-Allow-Origin`. |
| `FILELOFT_CORS_ALLOW_CREDENTIALS` | `false` | `Access-Control-Allow-Credentials`. |
| `FILELOFT_CORS_ALLOW_HEADERS` | _empty_ | Extra comma-separated names merged into preflight allow-headers. |
| `FILELOFT_CORS_EXPOSE_HEADERS` | _empty_ | Extra comma-separated names merged into expose-headers. |
| `FILELOFT_CORS_MAX_AGE` | `86400` | Preflight `Access-Control-Max-Age` (seconds). |
| `FILELOFT_DISABLE_TERMINATION` | `false` | Disable the termination extension (no `DELETE`). |
| `FILELOFT_ENABLE_DOWNLOAD` | `false` | Allow `GET` on upload URLs to download completed data. |
| `FILELOFT_ENABLE_CONCATENATION` | `false` | Enable concatenation extension. |
| `FILELOFT_ENABLE_CLEANUP_CONCAT_PARTIALS` | `false` | Delete partials after successful final concat. |
| `FILELOFT_ENABLE_EXPIRATION` | `false` | Enable expiration extension. |
| `FILELOFT_EXPIRATION_TTL` | _unset_ | TTL for incomplete uploads (seconds) when expiration is on. |
| `FILELOFT_DISABLE_CHECKSUM` | `false` | Disable checksum extension. |
| `FILELOFT_DISABLE_CREATION` | `false` | Disable creation (POST). |
| `FILELOFT_DISABLE_CREATION_WITH_UPLOAD` | `false` | Disable creation-with-upload. |
| `FILELOFT_DISABLE_DEFER_LENGTH` | `false` | Disable defer-length. |
| `FILELOFT_TLS_CERT` / `FILELOFT_TLS_KEY` | _unset_ | PEM paths for HTTPS; when both are set, the server listens with TLS on `FILELOFT_BIND`. |
| `FILELOFT_TLS_MODE` | `tls12` | Reserved for future Rustls cipher/protocol tuning (currently logs a warning if not `tls12`). |
| `FILELOFT_ENABLE_H2C` | `false` | When `true`, logs that HTTP/2 is available (workspace `axum` is built with `http2`). |

See the docs site for the full per-backend configuration reference.

Quick health check with any tus 1.0.0 client:

```bash
curl -i -X POST http://localhost:8080/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: 11" \
  -H "Upload-Metadata: filename aGVsbG8udHh0"
```

A `201 Created` response with a `Location` header indicates the endpoint is ready.

### Build and test from this repository

```bash
cargo test --workspace
```

The repository includes a **Makefile** for common tasks (`make help` lists targets). Useful shortcuts:

| Target | Purpose |
| --- | --- |
| `make setup` | Fetch Rust crates and install npm deps for the e2e Uppy bundle |
| `make test-unit` | Unit tests only (excludes integration, e2e, and server crates) |
| `make test-integration` | `fileloft-integration-tests` |
| `make test-e2e` | Browser e2e tests (see below) |
| `make test-all` | Unit, then integration, then e2e |
| `make e2e-server` | Build assets and run the Uppy + tus demo at [http://localhost:3000](http://localhost:3000) for manual checks |

#### End-to-end (browser) tests

The `fileloft-e2e-uppy` crate runs **ignored** tests that drive headless Chrome via **ChromeDriver**. You need [Google Chrome](https://www.google.com/chrome/) and a **ChromeDriver whose major version matches Chrome** (mismatches fail with a session error).

From the repo root, **`make test-e2e`** is the recommended entry point: it builds the vendored Uppy assets, then starts ChromeDriver on port **9515** if nothing is already listening on that port, and runs the tests. If you prefer to run ChromeDriver yourself, start it (`chromedriver --port=9515`) and run `cargo test -p fileloft-e2e-uppy -- --ignored`; set `WEBDRIVER_URL` if you use another address.

Details, asset rebuild, and macOS notes: [`crates/fileloft-e2e-uppy/README.md`](crates/fileloft-e2e-uppy/README.md).

The [Hugo](https://gohugo.io/) documentation site lives under `docs-site`; build it with:

```bash
cd docs-site && hugo --minify
```

## Contributing

- **Issues and PRs** — open an issue to discuss larger changes; pull requests are welcome for fixes and improvements that match the project’s scope (tus protocol implementation, adapters, and stores).
- **Checks** — before submitting, run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace` so CI stays green. This repo pins a Rust toolchain with `rustfmt` and `clippy` in `rust-toolchain.toml`.
- **Style** — follow existing patterns in the crate you touch; the workspace forbids `unsafe_code`. Prefer explicit error handling over panicking in library and application code.
- **Documentation** — user-facing behavior worth explaining should be reflected in crate docs or the `docs-site` content when it affects how people integrate or operate fileloft.

Licensed under the MIT license (see crate metadata).
