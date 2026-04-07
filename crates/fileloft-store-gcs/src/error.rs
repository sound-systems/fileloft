use thiserror::Error;

/// Errors from [`GcsStore`](crate::GcsStore) construction.
///
/// These represent configuration and SDK initialization failures that happen
/// at startup — before any tus request is processed.  Once the store is built,
/// all runtime errors go through [`TusError`](fileloft_core::error::TusError).
#[derive(Debug, Error)]
pub enum GcsStoreError {
    #[error("GCS bucket name is empty")]
    BucketEmpty,

    #[error("failed to build GCS client: {0}")]
    ClientInit(String),
}
