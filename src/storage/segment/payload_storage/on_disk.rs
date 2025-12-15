use serde_json::Value;
use std::collections::HashMap;

use crate::{
    error::StorageError,
    storage::segment::{payload_storage::PayloadStorageTrait, PointId},
};

pub struct OnDiskPayloadStorage {
    db: sled::Db,
    // Todo: Would be faster to use ahashmap here
    payload: HashMap<PointId, Value>,
}

impl OnDiskPayloadStorage {
    pub fn new(path: &std::path::Path) -> Result<Self, crate::error::StorageError> {
        let db_config = sled::Config::default()
            .path(path)
            .cache_capacity(1024 * 1024 * 1024) // 1GB cache
            // .use_compression(true)
            .print_profile_on_drop(true);

        let db = db_config.open().map_err(|e| {
            StorageError::ServiceError(format!("Failed to open on-disk payload storage: {e}"))
        })?;

        // Load existing payloads into memory
        let mut payload = HashMap::new();
        for result in db.iter() {
            let (key, value) = result.map_err(|e| {
                StorageError::ServiceError(format!("Failed to iterate over payload storage: {e}"))
            })?;
            let point = crate::storage::segment::Point::decode(&key, &value)?;
            payload.insert(point.id, point.payload);
        }

        Ok(Self { db, payload })
    }
}

impl PayloadStorageTrait for OnDiskPayloadStorage {
    fn len(&self) -> usize {
        self.db.len()
    }

    fn flush(&self) -> Result<(), StorageError> {
        self.db.flush().map_err(|e| {
            StorageError::ServiceError(format!("Failed to flush payload storage: {e}"))
        })?;
        Ok(())
    }

    fn insert(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StorageError> {
        self.db.insert(key, value).map_err(|e| {
            StorageError::ServiceError(format!("Failed to insert into payload storage: {e}"))
        })?;
        Ok(())
    }

    fn _inner_db(&self) -> &sled::Db {
        &self.db
    }
}
