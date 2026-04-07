<p align="center">
  <img src="docs-site/static/logo.png" alt="fileloft" width="240" />
</p>

# fileloft

**fileloft** is a Rust implementation of the [tus](https://tus.io) resumable upload protocol. It ships as a small set of composable crates you can embed in an existing HTTP server, or run as a standalone tus endpoint via the published container image.

- **Framework agnostic** — protocol core with no transport assumptions, plus adapters for Axum, Actix Web, and Rocket.
- **Pluggable storage** — a `DataStore` trait with in-memory and filesystem backends; bring your own for object storage or other backends.
- **Standalone or embedded** — use it as a library or run the prebuilt image when you only need a tus endpoint.
- **Safe by default** — `#![forbid(unsafe_code)]` across the workspace, with conservative defaults for limits, locking, and checksums.

Crates: `fileloft-core`, `fileloft-store-memory`, `fileloft-store-fs`, `fileloft-axum`, `fileloft-actix`, `fileloft-rocket`.

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

```bash
docker run --rm \
  -p 8080:8080 \
  -v fileloft-data:/var/lib/fileloft \
  ghcr.io/sound-systems/fileloft:latest
```

The server listens on `:8080` and stores data under `/var/lib/fileloft`. Common environment overrides:

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_BIND` | `0.0.0.0:8080` | Address the HTTP server binds to. |
| `FILELOFT_DATA_DIR` | `/var/lib/fileloft` | Directory used by the filesystem store. |
| `FILELOFT_MAX_SIZE` | _unset_ | Maximum allowed upload size, in bytes. |
| `FILELOFT_BASE_PATH` | `/files` | Path the tus endpoints are mounted under. |

More detail lives in the Hugo site under `docs-site`.

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
