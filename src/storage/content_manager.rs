use crate::storage::error::StorageError;
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

pub const COLLECTIONS_DIR: &str = "collections";
pub const COLLECTION_CONFIG_FILE: &str = "config.json";

pub struct TableOfContent {
    pub collections: Arc<RwLock<Collections>>,
}

#[derive(Serialize)]
pub struct CollectionConfig {
    pub params: String,
}

impl CollectionConfig {
    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let config_path = path.join(COLLECTION_CONFIG_FILE);
        let serde_json_bytes = serde_json::to_vec(self).map_err(|e| {
            StorageError::BadInput(format!(
                "Failed to serialize collection config to JSON: {}",
                e
            ))
        })?;

        let mut file = std::fs::File::create(config_path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to create collection config file: {}", e))
        })?;

        // Use buffered write for higher perf otherwise it will do multiple kernel calls
        std::io::BufWriter::new(&mut file)
            .write_all(&serde_json_bytes)
            .map_err(|e| {
                StorageError::ServiceError(format!("Failed to write collection config: {}", e))
            })?;

        Ok(())
    }
}

pub type CollectionId = String;

#[derive(Serialize)]
pub struct Collection {
    pub id: CollectionId,
    pub config: CollectionConfig,
    pub path: PathBuf,
}

impl Collection {
    pub fn init(
        id: CollectionId,
        config: CollectionConfig,
        path: &Path,
    ) -> Result<Self, StorageError> {
        // ToDo: Create ShardHolder & ShardReplicaSet

        config.save(path)?;

        Ok(Collection {
            id,
            config,
            path: path.to_owned(),
        })
    }
}

pub type Collections = HashMap<CollectionId, Collection>;

pub enum CollectionMetaOperation {
    CreateCollection {
        collection_name: String,
        params: String,
    },
}

impl TableOfContent {
    pub fn from(collections: Collections) -> Self {
        TableOfContent {
            collections: Arc::new(RwLock::new(collections)),
        }
    }

    /// Creates a new directory at the expected collection path.
    pub async fn mkdir_collection_dir(
        &self,
        collection_name: &str,
    ) -> Result<PathBuf, StorageError> {
        let path = Path::new("storage")
            .join(COLLECTIONS_DIR)
            .join(collection_name);

        if path.exists() {
            return Err(StorageError::BadInput(format!(
                "Collection path already exists: {}",
                path.display()
            )));
        }

        tokio::fs::create_dir_all(&path).await.map_err(|e| {
            StorageError::ServiceError(format!("Can't create directory for collection: {}", e))
        })?;

        Ok(path)
    }

    pub async fn perform_collection_meta_op(
        &self,
        operation: CollectionMetaOperation,
    ) -> Result<bool, StorageError> {
        match operation {
            CollectionMetaOperation::CreateCollection {
                collection_name,
                params,
            } => {
                println!("Creating collection {}", collection_name);
                let path = self.mkdir_collection_dir(&collection_name).await?;

                let collection =
                    Collection::init(collection_name.clone(), CollectionConfig { params }, &path)?;

                {
                    let mut write_collections = self.collections.write().await;
                    if write_collections.contains_key(&collection_name) {
                        return Err(StorageError::BadInput(format!(
                            "Collection with name '{}' already exists",
                            collection_name
                        )));
                    }
                    write_collections.insert(collection_name, collection);
                }
                Ok(true)
            }
        }
    }
}
