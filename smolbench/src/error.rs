use thiserror::Error;

// Qdrant related errors
#[derive(Error, Debug)]
pub enum SmolBenchError {
    #[error("Failed to create collection: {0}")]
    CreateCollectionError(String),
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
}
