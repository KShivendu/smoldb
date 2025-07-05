use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Bad input: {0}")]
    BadInput(String),
    #[error("Service error: {0}")]
    ServiceError(String),
}

#[derive(Error, Debug)]
pub enum CollectionError {
    #[error("Tonic transport error: {0}")]
    TonicTransportError(#[from] tonic::transport::Error),
    #[error("Tonic error: {0}")]
    TonicStatusError(#[from] tonic::Status),
    #[error("Service error: {0}")]
    ServiceError(String),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
}

pub type CollectionResult<T> = Result<T, CollectionError>;
