use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Bad input: {0}")]
    BadInput(String),
    #[error("Service error: {0}")]
    ServiceError(String),
    #[error("Encoding/Decoding error: {0}")]
    CodecError(String),
    #[error("Sled segment error: {0}")]
    SledError(#[from] sled::Error),
    #[error("Serialization/Deserialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Consensus is not enabled")]
    NotEnabled,
    #[error("Consensus is not ready yet")]
    NotReady,
    #[error("Service error: {0}")]
    ServiceError(String),
}

#[derive(Error, Debug)]
pub enum CollectionError {
    #[error("Tonic transport error: {0}")]
    TonicTransportError(#[from] tonic::transport::Error),
    #[error("Tonic error: {0}")]
    TonicStatusError(#[from] Box<tonic::Status>), // tonic::Status is 176B and bloats the error size. so we box it
    #[error("Service error: {0}")]
    ServiceError(String),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Consensus error: {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("Json parsing error: {0}")]
    JsonParseError(#[from] serde_path_to_error::Error<serde_json::Error>),
}

// Need special implementation for tonic::Status to wrap into Box
impl From<tonic::Status> for CollectionError {
    fn from(status: tonic::Status) -> Self {
        CollectionError::TonicStatusError(Box::new(status))
    }
}

pub type CollectionResult<T> = Result<T, CollectionError>;
pub type StorageResult<T> = Result<T, StorageError>;
