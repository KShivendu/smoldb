use crate::storage::error::StorageError;
use std::path::PathBuf;

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, data storage, index, etc
}

impl Segment {
    pub fn create(segments_dir: &PathBuf) -> Result<Self, StorageError> {
        // ToDo: Have uuid segment ID
        let path = segments_dir.join("0");
        std::fs::create_dir_all(&path).expect("Failed to create segment directory");

        let db = sled::open(&path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to open sled database: {}", e))
        })?;

        Ok(Self { path, db })
    }

    pub fn load(path: &PathBuf) -> Result<Self, StorageError> {
        if !path.exists() {
            return Err(StorageError::ServiceError(format!(
                "Segment path does not exist: {:?}",
                path
            )));
        }

        let db = sled::open(&path).expect("Failed to open sled database");

        Ok(Self {
            path: path.to_owned(),
            db,
        })
    }
}
