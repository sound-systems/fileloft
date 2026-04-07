use thiserror::Error;

/// Errors from [`AzureStore`](crate::AzureStore) construction.
///
/// Runtime failures after the store is built surface as [`TusError`](fileloft_core::error::TusError).
#[derive(Debug, Error)]
pub enum AzureStoreError {
    #[error("Azure Blob container name is empty")]
    ContainerEmpty,

    #[error("storage account name is missing (set it on the builder or AZURE_STORAGE_ACCOUNT)")]
    AccountMissing,

    #[error("failed to build Azure Blob Storage client: {0}")]
    ClientInit(String),
}
