mod on_disk;

use on_disk::OnDiskPayloadStorage;

use crate::error::StorageError;

pub enum PayloadStorage {
    OnDiskPayloadStorage(OnDiskPayloadStorage),
}

impl PayloadStorage {
    pub fn new_on_disk(path: &std::path::Path) -> Result<Self, StorageError> {
        let storage = OnDiskPayloadStorage::new(path)?;
        Ok(PayloadStorage::OnDiskPayloadStorage(storage))
    }
}

pub trait PayloadStorageTrait {
    fn count(&self) -> usize;
    fn flush(&self) -> Result<(), StorageError>;
    fn insert(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StorageError>;
    fn iter(&self) -> sled::Iter {
        self._inner_db().iter()
    }

    // Todo: Don't expose sled db directly
    fn _inner_db(&self) -> &sled::Db;
}

impl PayloadStorageTrait for PayloadStorage {
    fn count(&self) -> usize {
        match self {
            PayloadStorage::OnDiskPayloadStorage(storage) => storage.count(),
        }
    }

    fn flush(&self) -> Result<(), StorageError> {
        match self {
            PayloadStorage::OnDiskPayloadStorage(storage) => storage.flush(),
        }
    }

    fn insert(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StorageError> {
        match self {
            PayloadStorage::OnDiskPayloadStorage(storage) => storage.insert(key, value),
        }
    }

    fn _inner_db(&self) -> &sled::Db {
        match self {
            PayloadStorage::OnDiskPayloadStorage(storage) => storage._inner_db(),
        }
    }
}
