use std::time::Duration;

use crate::hooks::HookConfig;

/// Runtime flags controlling which tus protocol extensions are active.
///
/// These are runtime config, not Cargo features, so one compiled binary can
/// serve any combination without recompilation.
#[derive(Debug, Clone)]
pub struct Extensions {
    /// POST to create a new upload (required by most clients).
    pub creation: bool,
    /// POST with body to combine creation and first chunk.
    pub creation_with_upload: bool,
    /// Allow `Upload-Defer-Length: 1` to defer declaring size at creation.
    pub creation_defer_length: bool,
    /// Attach expiry timestamps to incomplete uploads.
    pub expiration: bool,
    /// How long incomplete uploads live before expiry. Used when `expiration = true`.
    pub expiration_ttl: Option<Duration>,
    /// Validate chunk integrity via `Upload-Checksum` header.
    pub checksum: bool,
    /// Accept checksum in HTTP trailers (requires HTTP/1.1 chunked or HTTP/2).
    pub checksum_trailer: bool,
    /// Allow clients to DELETE uploads.
    pub termination: bool,
    /// Allow parallel partial uploads assembled into a final upload.
    pub concatenation: bool,
    /// After a successful `final` concatenation, delete partial upload resources.
    pub cleanup_concat_partials: bool,
}

impl Default for Extensions {
    fn default() -> Self {
        Self {
            creation: true,
            creation_with_upload: true,
            creation_defer_length: true,
            expiration: false,
            expiration_ttl: None,
            checksum: true,
            checksum_trailer: false,
            termination: true,
            concatenation: false,
            cleanup_concat_partials: false,
        }
    }
}

/// Top-level handler configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// URL path prefix where upload resources are rooted, e.g. `"/files/"`.
    /// Used to build the `Location` header value on POST responses.
    pub base_path: String,
    /// Optional absolute base URL override (e.g. `"https://uploads.example.com"`).
    /// When `None` the handler builds the Location from the request's `Host` header.
    pub base_url: Option<String>,
    /// Maximum allowed `Upload-Length` in bytes. `0` means no server-imposed limit.
    pub max_size: u64,
    /// Enabled protocol extensions.
    pub extensions: Extensions,
    /// How long to wait when acquiring a per-upload lock before returning 408.
    pub lock_timeout: Duration,
    /// Add permissive CORS headers to every response.
    pub enable_cors: bool,
    /// When `base_url` is unset, trust `X-Forwarded-Proto` / `X-Forwarded-Host` for `Location`.
    /// Enable only behind a trusted reverse proxy.
    pub trust_forwarded_headers: bool,
    /// Hook callbacks and event-channel configuration.
    pub hooks: HookConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_path: "/files/".to_string(),
            base_url: None,
            max_size: 0,
            extensions: Extensions::default(),
            lock_timeout: Duration::from_secs(20),
            enable_cors: false,
            trust_forwarded_headers: false,
            hooks: HookConfig::default(),
        }
    }
}
