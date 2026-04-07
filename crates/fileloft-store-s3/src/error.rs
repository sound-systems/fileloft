use thiserror::Error;

/// Errors from [`S3Store`](crate::S3Store) construction.
///
/// These represent configuration and SDK initialization failures that happen
/// at startup — before any tus request is processed. Once the store is built,
/// all runtime errors go through [`TusError`](fileloft_core::error::TusError).
#[derive(Debug, Error)]
pub enum S3StoreError {
    #[error("S3 bucket name is empty")]
    BucketEmpty,

    #[error("failed to build S3 client: {0}")]
    ClientInit(String),
}
