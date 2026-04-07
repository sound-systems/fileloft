pub const TUS_VERSION: &str = "1.0.0";

// Request/response header names
pub const HDR_TUS_RESUMABLE: &str = "Tus-Resumable";
pub const HDR_TUS_VERSION: &str = "Tus-Version";
pub const HDR_TUS_EXTENSION: &str = "Tus-Extension";
pub const HDR_TUS_MAX_SIZE: &str = "Tus-Max-Size";
pub const HDR_TUS_CHECKSUM_ALGORITHM: &str = "Tus-Checksum-Algorithm";
pub const HDR_UPLOAD_OFFSET: &str = "Upload-Offset";
pub const HDR_UPLOAD_LENGTH: &str = "Upload-Length";
pub const HDR_UPLOAD_DEFER_LENGTH: &str = "Upload-Defer-Length";
pub const HDR_UPLOAD_METADATA: &str = "Upload-Metadata";
pub const HDR_UPLOAD_EXPIRES: &str = "Upload-Expires";
pub const HDR_UPLOAD_CHECKSUM: &str = "Upload-Checksum";
pub const HDR_UPLOAD_CONCAT: &str = "Upload-Concat";
pub const HDR_LOCATION: &str = "Location";
pub const HDR_CACHE_CONTROL: &str = "Cache-Control";
pub const HDR_CONTENT_TYPE: &str = "Content-Type";
pub const HDR_CONTENT_LENGTH: &str = "Content-Length";
pub const HDR_ACCESS_CONTROL_ALLOW_ORIGIN: &str = "Access-Control-Allow-Origin";
pub const HDR_ACCESS_CONTROL_ALLOW_METHODS: &str = "Access-Control-Allow-Methods";
pub const HDR_ACCESS_CONTROL_ALLOW_HEADERS: &str = "Access-Control-Allow-Headers";
pub const HDR_ACCESS_CONTROL_EXPOSE_HEADERS: &str = "Access-Control-Expose-Headers";
pub const HDR_ACCESS_CONTROL_MAX_AGE: &str = "Access-Control-Max-Age";

pub const CONTENT_TYPE_OCTET_STREAM: &str = "application/offset+octet-stream";
pub const CACHE_CONTROL_NO_STORE: &str = "no-store";

/// RFC 9110 HTTP date format used for Upload-Expires
pub const HTTP_DATE_FORMAT: &str = "%a, %d %b %Y %H:%M:%S GMT";

/// All extension identifiers defined by tus 1.0.x
pub const EXT_CREATION: &str = "creation";
pub const EXT_CREATION_WITH_UPLOAD: &str = "creation-with-upload";
pub const EXT_CREATION_DEFER_LENGTH: &str = "creation-defer-length";
pub const EXT_EXPIRATION: &str = "expiration";
pub const EXT_CHECKSUM: &str = "checksum";
pub const EXT_CHECKSUM_TRAILER: &str = "checksum-trailer";
pub const EXT_TERMINATION: &str = "termination";
pub const EXT_CONCATENATION: &str = "concatenation";

pub const SUPPORTED_CHECKSUM_ALGORITHMS: &[&str] = &["sha1", "sha256", "md5"];
