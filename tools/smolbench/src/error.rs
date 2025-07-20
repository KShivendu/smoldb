use thiserror::Error;

#[derive(Error, Debug)]
pub enum SmolBenchError {
    #[error("Failed to create collection: {0}")]
    CreateCollectionError(String),
    #[error("Failed to check if collection exists: {0}")]
    CollectionExistsError(String),
    #[error("Failed to delete collection: {0}")]
    DeleteCollectionError(String),
    #[error("Failed to upsert points: {0}")]
    UpsertPointsError(String),
    #[error("Failed to read points: {0}")]
    ReadPointsError(String),
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
}
