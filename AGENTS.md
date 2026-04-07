# fileloft — agent context

Use this document when working in this repository: it summarizes purpose, layout, architecture, testing, and conventions.

## What this project is

**fileloft** is a Rust implementation of the [tus](https://tus.io) resumable upload protocol (tus 1.0.0 core). It is meant to be embedded in existing HTTP servers or used behind a container image. The design is **framework-agnostic at the core**: protocol logic lives in `fileloft-core`; HTTP frameworks only adapt requests/responses.

## Repository layout (workspace)

| Path | Role |
| --- | --- |
| `crates/fileloft-core` | Tus protocol engine: `TusHandler`, `TusRequest` / `TusResponse`, `Config`, `TusError`, storage and lock traits. **Published.** |
| `crates/fileloft-store-memory` | In-memory `DataStore` (tests, demos). **Published.** |
| `crates/fileloft-store-fs` | Filesystem store + `FileLocker`. **Published.** |
| `crates/fileloft-store-gcs`, `fileloft-store-s3`, `fileloft-store-azure` | Object-storage backends. **Published** (cloud-specific deps). |
| `crates/fileloft-axum`, `fileloft-actix`, `fileloft-rocket` | Adapters: build a `TusHandler`, expose framework-specific routers. **Published.** |
| `crates/fileloft-server` | **Not published.** Standalone Axum binary with Cargo features `backend-fs` / `backend-s3` / `backend-gcs` / `backend-azure`. Reads `FILELOFT_*` env vars. |
| `crates/fileloft-integration-tests` | **Not published.** Black-box HTTP tests against the core handler + memory store (tus flows: POST, PATCH, HEAD, DELETE, extensions). |
| `crates/fileloft-e2e-uppy` | **Not published.** Axum demo app + vendored Uppy bundle + optional headless WebDriver e2e (`#[ignore]`). |
| `docs-site/` | Hugo site for user-facing documentation. |
| `Dockerfile` | Multi-stage build; `ARG BACKEND=fs` selects the storage backend. |
| `Makefile` | Developer ergonomics: `help`, `setup`, `test-unit`, `test-integration`, `test-e2e`, `test-all`, `docker-build-*`. |

Root `Cargo.toml` is the workspace manifest; internal crates use `path` + version for `cargo publish`.

## Architecture (how it fits together)

1. **Storage model** — `DataStore` creates and opens `Upload` handles. `Upload` implements chunk writes, finalize, delete, deferred length, and concatenation. Implementations return `TusError::ExtensionNotEnabled` when an optional feature is not supported.

2. **Handler** — `TusHandler<S, L>` owns the store `S`, optional `Locker` `L` (or `NoLocker` when locking is unnecessary), and `Config`. The **`handle` method** dispatches by HTTP method: `OPTIONS`, `HEAD`, `POST`, `PATCH`, `DELETE`. Framework crates convert native requests into `TusRequest` (method, URI, upload id from path, headers, optional streaming body) and map `TusResponse` back.

3. **Configuration** — `Config` + `Extensions` control base URL/path, size limits, CORS, lock timeouts, forwarded-header trust, hooks, and which tus **extensions** are enabled (creation, creation-with-upload, defer-length, expiration, checksum, termination, concatenation, etc.). Extensions are **runtime** flags, not Cargo features, so one binary can enable combinations without recompiling for each.

4. **Safety** — The workspace sets `#![forbid(unsafe_code)]` in crate attributes; keep it that way.

## Design principles (when changing code)

- **No `unsafe`.** Prefer clear, typed APIs.
- **Errors, not panics** — Library and integration code should use `Result` and `TusError` (or appropriate errors in binaries). Do not add `unwrap()`/`expect()` in library paths; use the project’s existing error-handling style.
- **Thin framework layers** — Adapters should map types and call `TusHandler::handle`; protocol decisions stay in `fileloft-core`.
- **Conservative defaults** — Respect existing limits, locking, and checksum behavior unless a change is explicitly required.
- **Match surrounding style** — Naming, module structure, and documentation level should match the crate you edit.

## How to test

**Toolchain:** `rust-toolchain.toml` pins stable Rust with `rustfmt` and `clippy`.

**Makefile (preferred for labels and split suites):**

| Target | What it runs |
| --- | --- |
| `make setup` | `cargo fetch` + `npm ci` in `crates/fileloft-e2e-uppy` (JS deps for bundling Uppy assets). |
| `make test-unit` | Workspace tests **excluding** `fileloft-integration-tests`, `fileloft-e2e-uppy`, and `fileloft-server` (in-crate / unit tests). |
| `make test-integration` | `cargo test -p fileloft-integration-tests`. |
| `make test-e2e` | `cargo test -p fileloft-e2e-uppy -- --ignored` — requires Chrome and a **matching** ChromeDriver (e.g. `chromedriver --port=9515`). |
| `make test-all` | Runs unit, then integration, then e2e. |

**CI (`.github/workflows/ci.yml`):** `cargo test --workspace` (everything including integration tests; e2e ignored tests are not run in that job unless CI is extended).

**Full workspace test:** `cargo test --workspace` (same as CI; ignored e2e tests still skipped unless `-- --ignored`).

**Pre-submit checks** (from `README.md` / contributing): `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and tests as above.

## E2E / Uppy assets

The `fileloft-e2e-uppy` crate vendors a browser bundle under `static/vendor/` so tests do not rely on a CDN. Regenerating that bundle: see `crates/fileloft-e2e-uppy/README.md` (`npm ci`, `npm run build` after dependency or entrypoint changes).

## Documentation

- **API:** Rustdoc on crates (`docs.rs` for published crates).
- **Site:** `docs-site/` (Hugo).

## Container image

The repository includes a `Dockerfile` at the root and `Makefile` targets for building per-backend images. The `BACKEND` build arg (`fs`, `s3`, `gcs`, `azure`) selects which Cargo feature is compiled into the binary. Images are published to `ghcr.io/sound-systems/fileloft` with tag suffixes matching the backend name (`:latest` / `:fs`, `:s3`, `:gcs`, `:azure`).
