use crate::{
    api::points::PointsOperation,
    storage::{
        error::StorageError,
        segment::{Point, PointId},
        shard::{LocalShard, ShardId},
    },
};
use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize, Clone)]
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

pub struct Collection {
    pub id: CollectionId,
    pub config: CollectionConfig,
    pub shards: HashMap<ShardId, LocalShard>,
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

        // ToDo: Initialize with more shards and have remote shards
        let s0_path = path.join("0");
        let shards = HashMap::from_iter([(0, LocalShard::init(s0_path, 0))]);

        Ok(Collection {
            id,
            config,
            shards,
            path: path.to_owned(),
        })
    }

    pub fn load(id: CollectionId, path: &Path) -> Result<Self, StorageError> {
        let config_path = path.join(COLLECTION_CONFIG_FILE);
        if !config_path.exists() {
            return Err(StorageError::BadInput(format!(
                "Collection config file does not exist at path: {}",
                config_path.display()
            )));
        }

        let config: CollectionConfig = {
            let config_file = std::fs::File::open(&config_path).map_err(|e| {
                StorageError::ServiceError(format!("Failed to open collection config file: {}", e))
            })?;

            let config_file_buf = std::io::BufReader::new(&config_file);
            serde_json::from_reader(config_file_buf).map_err(|e| {
                StorageError::BadInput(format!("Failed to parse collection config JSON: {}", e))
            })?
        };

        // ToDo: Load shards
        let mut shards = HashMap::new();
        let dir_contents = std::fs::read_dir(path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to read collection directory: {}", e))
        })?;

        for entry in dir_contents {
            let path = entry.expect("Can't read directory entry").path();
            if !path.is_dir() {
                continue; // Skip non-directory entries
            }

            let shard = LocalShard::load(&path)?;
            shards.insert(shard.id, shard);
        }

        Ok(Collection {
            id,
            config,
            shards,
            path: path.to_path_buf(),
        })
    }

    pub fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        // ToDo: Add shard routing with hashring?
        let Some((_, shard)) = self.shards.iter().next() else {
            return Err(StorageError::BadInput(format!(
                "Collection '{}' has no shards",
                self.id
            )));
        };

        shard.insert_points(&points).map_err(|e| {
            StorageError::ServiceError(format!(
                "Failed to upsert points in collection '{}': {}",
                self.id, e
            ))
        })?;

        Ok(())
    }

    pub fn get_points(&self, ids: &[PointId]) -> Result<Vec<Point>, StorageError> {
        // ToDo: Add shard routing with hashring?
        let Some((_, shard)) = &self.shards.iter().next() else {
            return Err(StorageError::BadInput(format!(
                "Collection '{}' has no shards",
                self.id
            )));
        };

        shard.get_points(ids).map_err(|e| {
            StorageError::ServiceError(format!(
                "Failed to retrieve points from collection '{}': {}",
                self.id, e
            ))
        })
    }
}

#[derive(Serialize)]
pub struct CollectionInfo {
    pub id: CollectionId,
    pub config: CollectionConfig,
    pub shard_count: usize,
    pub segment_count: usize,
}

impl From<&Collection> for CollectionInfo {
    fn from(collection: &Collection) -> Self {
        CollectionInfo {
            id: collection.id.clone(),
            config: collection.config.clone(),
            shard_count: collection.shards.len(),
            segment_count: collection
                .shards
                .values()
                .map(|shard| shard.segments.len())
                .sum(),
        }
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

    pub fn load() -> Self {
        let collections_path = Path::new("storage").join(COLLECTIONS_DIR);
        std::fs::create_dir_all(&collections_path).expect("Failed to create collections directory");

        // Load collections from the directory
        let mut collections = HashMap::new();
        let collection_paths =
            std::fs::read_dir(&collections_path).expect("Failed to read collections directory");
        for dir_entry in collection_paths {
            let path = dir_entry.expect("Can't read directory entry").path();

            if !path.join(COLLECTION_CONFIG_FILE).exists() {
                // Skip directories without a config file
                // This indirectly also checks if the path is a directory
                println!(
                    "Skipping path {} as it does not contain a collection config file",
                    path.display()
                );
                continue;
            }

            let collection_name = path
                .file_name()
                .expect("Can't resolve filename for collection")
                .to_str()
                .expect("Collection name is not valid UTF-8")
                .to_string();

            let collection = Collection::load(collection_name, &path)
                .expect("Failed to load collection from path");

            collections.insert(collection.id.clone(), collection);
        }

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

    pub async fn perform_points_op(
        &self,
        collection_name: &str,
        operation: PointsOperation,
    ) -> Result<bool, StorageError> {
        // ToDo: Have independent read locks for each collection. It should improve perf?
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{}' does not exist", collection_name))
        })?;

        match operation {
            PointsOperation::Upsert(upsert_points) => {
                collection.insert_points(&upsert_points.points)?;
            }
        }

        Ok(true)
    }

    pub async fn retrieve_points(
        &self,
        collection_name: &str,
        ids: &[PointId],
    ) -> Result<Vec<Point>, StorageError> {
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{}' does not exist", collection_name))
        })?;

        collection.get_points(ids)
    }
}
