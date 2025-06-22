use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Bad input: {0}")]
    BadInput(String),
    #[error("Service error: {0}")]
    ServiceError(String),
}
