use crate::{
    error::TusError,
    info::{UploadId, UploadInfo},
};

/// Operations on a single upload resource.
#[trait_variant::make(SendUpload: Send)]
pub trait Upload {
    /// Write chunk data starting at `offset`, streaming from `reader`.
    /// Returns the number of bytes written.
    async fn write_chunk(
        &mut self,
        offset: u64,
        reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError>;

    /// Retrieve current metadata and offset.
    async fn get_info(&self) -> Result<UploadInfo, TusError>;

    /// Called once all bytes have been received (offset == size).
    async fn finalize(&mut self) -> Result<(), TusError>;

    /// Delete this upload and free all associated resources.
    /// Called by the termination extension (DELETE). Return `Err` if unsupported.
    async fn delete(self) -> Result<(), TusError>;

    /// Set the definitive `Upload-Length` on a deferred-length upload.
    /// Called when the client provides `Upload-Length` on a PATCH request.
    async fn declare_length(&mut self, length: u64) -> Result<(), TusError>;

    /// Assemble fully-uploaded partials (in order) into this final upload.
    /// Called by the concatenation extension.
    async fn concatenate(&mut self, partials: &[UploadInfo]) -> Result<(), TusError>;

    /// Stream the completed upload bytes for HTTP GET (download) requests.
    /// Return [`TusError::UploadNotReadyForDownload`] when the upload is incomplete or not readable.
    async fn read_content(&self) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, TusError>;
}

/// Core storage abstraction.
///
/// Implement this trait to plug in any persistence backend (filesystem, S3, etc.).
/// The associated `UploadType` must implement all upload operations; return
/// `TusError::ExtensionNotEnabled` from extension methods your store doesn't support.
#[trait_variant::make(SendDataStore: Send)]
pub trait DataStore {
    type UploadType: SendUpload;

    /// Create a new upload slot and return a handle to it.
    async fn create_upload(&self, info: UploadInfo) -> Result<Self::UploadType, TusError>;

    /// Retrieve an existing upload by ID.
    async fn get_upload(&self, id: &UploadId) -> Result<Self::UploadType, TusError>;
}
