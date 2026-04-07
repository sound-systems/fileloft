---
title: "Use as a library"
slug: "library"
weight: 2
---

fileloft is published as a workspace of small crates. The protocol logic lives
in `fileloft-core`; framework adapters and storage backends are separate crates so
you only pull in what you use.

### Add the dependencies

```toml
[dependencies]
fileloft-core         = "0.1"
fileloft-store-fs     = "0.1"
fileloft-axum         = "0.1"   # or fileloft-actix / fileloft-rocket
tokio                 = { version = "1", features = ["full"] }
```

### Mount it on your router

The example below uses Axum, but the shape is the same for the other
adapters — construct a `TusHandler` from a store, optional locker, and `Config`, then mount
the adapter's router under whatever path you like.

```rust
use std::sync::Arc;
use fileloft_core::{Config, TusHandler};
use fileloft_store_fs::{FileLocker, FileStore};
use fileloft_axum::tus_router;

#[tokio::main]
async fn main() {
    let root = "/var/lib/fileloft";
    let store = FileStore::new(root);
    let locker = FileLocker::new(format!("{root}/locks"));
    let handler = Arc::new(TusHandler::new(store, Some(locker), Config::default()));

    let app = axum::Router::new()
        .nest("/files", tus_router(handler));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

### What you get

- Full tus 1.0.0 core protocol: `creation`, `expiration`, `checksum`,
  `termination`, and `concatenation` extensions are opt-in via `Config`.
- A `DataStore` trait you can implement to back uploads with your own storage.
- A `HookSender` for observing upload lifecycle events without coupling to a
  specific message bus.

See the [crate docs on docs.rs](https://docs.rs/fileloft-core) for the full API.
