use std::time::Duration;

use crate::hooks::HookConfig;

/// Cross-origin resource sharing settings (tus clients in browsers).
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// When false, no CORS headers are added.
    pub enabled: bool,
    /// `Access-Control-Allow-Origin` value (e.g. `"*"` or `"https://example.com"`).
    pub allow_origin: String,
    /// `Access-Control-Allow-Credentials`.
    pub allow_credentials: bool,
    /// Extra header names merged into `Access-Control-Allow-Headers` (tus defaults are always included).
    pub extra_allow_headers: Vec<String>,
    /// Extra header names merged into `Access-Control-Expose-Headers` (tus defaults are always included).
    pub extra_expose_headers: Vec<String>,
    /// `Access-Control-Max-Age` for preflight (seconds).
    pub max_age: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_origin: "*".to_string(),
            allow_credentials: false,
            extra_allow_headers: Vec::new(),
            extra_expose_headers: Vec::new(),
            max_age: 86400,
        }
    }
}

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
    /// CORS headers on responses.
    pub cors: CorsConfig,
    /// When `base_url` is unset, use `X-Forwarded-Proto` / `X-Forwarded-Host` to build absolute
    /// URLs (e.g. behind TLS termination). **Only enable when this service is not directly exposed
    /// to untrusted clients** (forwarded headers can be spoofed).
    pub trust_forwarded_headers: bool,
    /// Hook callbacks and event-channel configuration.
    pub hooks: HookConfig,
    /// Allow HTTP GET on upload URLs to download completed data (tus-style downloads).
    /// When `false`, GET returns 405.
    pub enable_download: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_path: "/files/".to_string(),
            base_url: None,
            max_size: 0,
            extensions: Extensions::default(),
            lock_timeout: Duration::from_secs(20),
            cors: CorsConfig::default(),
            trust_forwarded_headers: false,
            hooks: HookConfig::default(),
            enable_download: false,
        }
    }
}
