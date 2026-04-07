use http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TusError {
    // --- Protocol-level errors ---
    #[error("missing Tus-Resumable header")]
    MissingTusResumable,

    #[error("unsupported tus version: {version}")]
    UnsupportedVersion { version: String },

    #[error("missing Upload-Offset header")]
    MissingUploadOffset,

    #[error("upload offset mismatch: expected {expected}, got {actual}")]
    OffsetMismatch { expected: u64, actual: u64 },

    #[error("wrong Content-Type: expected application/offset+octet-stream, got {0}")]
    WrongContentType(String),

    #[error("upload not found: {0}")]
    NotFound(String),

    #[error("upload has expired")]
    Gone,

    #[error("upload size exceeds server maximum of {max} bytes")]
    EntityTooLarge { max: u64 },

    #[error("chunk exceeds declared upload length (declared {declared} bytes, end offset would be {end})")]
    ExceedsUploadLength { declared: u64, end: u64 },

    #[error("checksum mismatch")]
    ChecksumMismatch,

    #[error("unsupported checksum algorithm: {0}")]
    UnsupportedChecksumAlgorithm(String),

    #[error("missing Upload-Length (or Upload-Defer-Length)")]
    MissingUploadLength,

    #[error("Upload-Length cannot be changed once set")]
    UploadLengthAlreadySet,

    #[error("extension not enabled: {0}")]
    ExtensionNotEnabled(&'static str),

    #[error("invalid metadata: {0}")]
    InvalidMetadata(String),

    #[error("invalid upload ID")]
    InvalidUploadId,

    #[error("concatenation requires at least one partial upload URL")]
    EmptyConcatenation,

    #[error("partial upload {0} is not yet complete")]
    PartialUploadIncomplete(String),

    #[error("PATCH is not allowed on a final concatenated upload")]
    PatchOnFinalUpload,

    #[error("method not allowed")]
    MethodNotAllowed,

    // --- Concurrency errors ---
    #[error("lock acquisition timed out for upload {0}")]
    LockTimeout(String),

    #[error("lock is already held for upload {0}")]
    LockConflict(String),

    // --- Hook errors ---
    #[error("hook rejected request: {0}")]
    HookRejected(String),

    // --- Storage / internal errors ---
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl TusError {
    /// Maps each variant to the appropriate HTTP status code per the tus 1.0.x spec.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingTusResumable => StatusCode::PRECONDITION_FAILED,
            Self::UnsupportedVersion { .. } => StatusCode::PRECONDITION_FAILED,
            Self::MissingUploadOffset => StatusCode::BAD_REQUEST,
            Self::OffsetMismatch { .. } => StatusCode::CONFLICT,
            Self::WrongContentType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Gone => StatusCode::GONE,
            Self::EntityTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::ExceedsUploadLength { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            // 460 is a non-standard tus status code; http crate accepts arbitrary codes.
            Self::ChecksumMismatch => match StatusCode::from_u16(460) {
                Ok(s) => s,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::UnsupportedChecksumAlgorithm(_) => StatusCode::BAD_REQUEST,
            Self::MissingUploadLength => StatusCode::BAD_REQUEST,
            Self::UploadLengthAlreadySet => StatusCode::BAD_REQUEST,
            Self::ExtensionNotEnabled(_) => StatusCode::NOT_FOUND,
            Self::InvalidMetadata(_) => StatusCode::BAD_REQUEST,
            Self::InvalidUploadId => StatusCode::BAD_REQUEST,
            Self::EmptyConcatenation => StatusCode::BAD_REQUEST,
            Self::PartialUploadIncomplete(_) => StatusCode::BAD_REQUEST,
            Self::PatchOnFinalUpload => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::LockTimeout(_) => StatusCode::REQUEST_TIMEOUT,
            Self::LockConflict(_) => StatusCode::LOCKED,
            Self::HookRejected(_) => StatusCode::FORBIDDEN,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Serialization(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every TusError variant must map to a well-defined HTTP status code.
    #[test]
    fn status_code_mapping() {
        let cases: &[(TusError, u16)] = &[
            (TusError::MissingTusResumable, 412),
            (
                TusError::UnsupportedVersion {
                    version: "0.9".into(),
                },
                412,
            ),
            (TusError::MissingUploadOffset, 400),
            (
                TusError::OffsetMismatch {
                    expected: 10,
                    actual: 5,
                },
                409,
            ),
            (TusError::WrongContentType("text/plain".into()), 415),
            (TusError::NotFound("abc".into()), 404),
            (TusError::Gone, 410),
            (TusError::EntityTooLarge { max: 1024 }, 413),
            (
                TusError::ExceedsUploadLength {
                    declared: 10,
                    end: 20,
                },
                413,
            ),
            (TusError::ChecksumMismatch, 460),
            (TusError::UnsupportedChecksumAlgorithm("crc32".into()), 400),
            (TusError::MissingUploadLength, 400),
            (TusError::UploadLengthAlreadySet, 400),
            (TusError::ExtensionNotEnabled("concatenation"), 404),
            (TusError::InvalidMetadata("bad base64".into()), 400),
            (TusError::InvalidUploadId, 400),
            (TusError::EmptyConcatenation, 400),
            (TusError::PartialUploadIncomplete("id1".into()), 400),
            (TusError::PatchOnFinalUpload, 403),
            (TusError::MethodNotAllowed, 405),
            (TusError::LockTimeout("id1".into()), 408),
            (TusError::LockConflict("id1".into()), 423),
            (TusError::HookRejected("not allowed".into()), 403),
            (TusError::Io(std::io::Error::other("disk full")), 500),
            (
                TusError::Serialization(serde_json::from_str::<()>("!").unwrap_err()),
                500,
            ),
            (TusError::Internal("oops".into()), 500),
        ];

        for (err, expected_status) in cases {
            assert_eq!(
                err.status_code().as_u16(),
                *expected_status,
                "wrong status for: {err}"
            );
        }
    }
}
