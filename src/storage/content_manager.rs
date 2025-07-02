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
    pub fn save(&self, collection_dir: &Path) -> Result<(), StorageError> {
        let config_path = collection_dir.join(COLLECTION_CONFIG_FILE);
        let serde_json_bytes = serde_json::to_vec(self).map_err(|e| {
            StorageError::BadInput(format!(
                "Failed to serialize collection config to JSON: {e}"
            ))
        })?;

        let mut file = std::fs::File::create(config_path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to create collection config file: {e}"))
        })?;

        // Use buffered write for higher perf otherwise it will do multiple kernel calls
        std::io::BufWriter::new(&mut file)
            .write_all(&serde_json_bytes)
            .map_err(|e| {
                StorageError::ServiceError(format!("Failed to write collection config: {e}"))
            })?;

        Ok(())
    }
}

pub type CollectionId = String;

pub struct ShardHolder {
    shards: HashMap<ShardId, LocalShard>,
    ring: hashring::HashRing<ShardId>,
}

impl ShardHolder {
    pub fn new(shards: HashMap<ShardId, LocalShard>) -> Self {
        let mut ring = hashring::HashRing::new();
        for shard_id in shards.keys() {
            ring.add(*shard_id);
        }

        ShardHolder { shards, ring }
    }

    pub fn empty() -> Self {
        ShardHolder {
            shards: HashMap::new(),
            ring: hashring::HashRing::new(),
        }
    }

    pub fn get_shard(&self, shard_id: ShardId) -> Result<&LocalShard, StorageError> {
        self.shards
            .get(&shard_id)
            .ok_or_else(|| StorageError::BadInput(format!("Shard {shard_id} not found")))
    }

    pub fn select_shards(
        &self,
        point_ids: &[PointId],
    ) -> Result<HashMap<ShardId, Vec<PointId>>, StorageError> {
        let mut shards_to_point_ids = HashMap::new();
        for point_id in point_ids {
            let shard_id = self.ring.get(&point_id).ok_or_else(|| {
                StorageError::ServiceError("No shards found".to_string())
            })?;

            shards_to_point_ids
                .entry(*shard_id)
                .or_insert_with(Vec::new)
                .push(point_id.clone());
        }
        Ok(shards_to_point_ids)
    }
}

pub struct Collection {
    pub id: CollectionId,
    pub config: CollectionConfig,
    pub shard_holder: Arc<RwLock<ShardHolder>>,
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
        let s1_path = path.join("1");
        let shards = HashMap::from_iter([
            (0, LocalShard::init(s0_path, 0)),
            (1, LocalShard::init(s1_path, 1)),
        ]);

        Ok(Collection {
            id,
            config,
            shard_holder: Arc::new(RwLock::new(ShardHolder::new(shards))),
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
                StorageError::ServiceError(format!("Failed to open collection config file: {e}"))
            })?;

            let config_file_buf = std::io::BufReader::new(&config_file);
            serde_json::from_reader(config_file_buf).map_err(|e| {
                StorageError::BadInput(format!("Failed to parse collection config JSON: {e}"))
            })?
        };

        // ToDo: Load shards
        let mut shards = HashMap::new();
        let dir_contents = std::fs::read_dir(path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to read collection directory: {e}"))
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
            shard_holder: Arc::new(RwLock::new(ShardHolder::new(shards))),
            path: path.to_path_buf(),
        })
    }

    pub async fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        let shard_holder = &self.shard_holder.read().await;

        let points_map: HashMap<PointId, Point> = points
            .iter()
            .map(|point| (point.id.clone(), point.clone()))
            .collect();

        let point_ids: Vec<_> = points.iter().map(|point| point.id.clone()).collect();

        // ToDo: Run this operation concurrently for each shard
        for (shard_id, shard_point_ids) in shard_holder.select_shards(&point_ids)? {
            let shard = shard_holder
                .shards
                .get(&shard_id)
                .ok_or_else(|| StorageError::BadInput(format!("Shard {shard_id} not found")))?;

            let points = shard_point_ids
                .iter()
                .filter_map(|id| points_map.get(id).cloned())
                .collect::<Vec<_>>();

            shard.insert_points(&points)?
        }

        Ok(())
    }

    pub async fn get_points(&self, ids: Option<&[PointId]>) -> Result<Vec<Point>, StorageError> {
        let shard_holder = self.shard_holder.read().await;

        let Some(ids) = ids else {
            // If no ids are provided, return all points from all shards
            let mut all_points = vec![];
            for shard in shard_holder.shards.values() {
                let points = shard.get_points(None)?;
                all_points.extend(points);
            }
            return Ok(all_points);
        };

        let mut points = vec![];

        for (shard_id, shard_point_ids) in shard_holder.select_shards(ids)? {
            let shard = shard_holder.get_shard(shard_id)?;

            let collected_points = shard.get_points(Some(&shard_point_ids))?;
            points.extend(collected_points);
        }

        Ok(points)
    }
}

#[derive(Serialize)]
pub struct CollectionInfo {
    pub id: CollectionId,
    pub config: CollectionConfig,
    pub shard_count: usize,
    pub segment_count: usize,
}

impl CollectionInfo {
    pub async fn from(collection: &Collection) -> Self {
        let shard_holder = collection.shard_holder.read().await;
        CollectionInfo {
            id: collection.id.clone(),
            config: collection.config.clone(),
            shard_count: shard_holder.shards.len(),
            segment_count: shard_holder
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
    pub async fn mkdir_collection_dir(collection_name: &str) -> Result<PathBuf, StorageError> {
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
            StorageError::ServiceError(format!("Can't create directory for collection: {e}"))
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
                println!("Creating collection {collection_name}");
                let path = Self::mkdir_collection_dir(&collection_name).await?;

                let collection =
                    Collection::init(collection_name.clone(), CollectionConfig { params }, &path)?;

                {
                    let mut write_collections = self.collections.write().await;
                    if write_collections.contains_key(&collection_name) {
                        return Err(StorageError::BadInput(format!(
                            "Collection with name '{collection_name}' already exists"
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
            StorageError::BadInput(format!("Collection '{collection_name}' does not exist"))
        })?;

        match operation {
            PointsOperation::Upsert(upsert_points) => {
                collection
                    .insert_points(&upsert_points.points)
                    .await
                    .map_err(|e| {
                        StorageError::ServiceError(format!(
                            "Failed to upsert points in collection '{collection_name}': {e}"
                        ))
                    })?;
            }
        }

        Ok(true)
    }

    pub async fn retrieve_points(
        &self,
        collection_name: &str,
        ids: Option<&[PointId]>,
    ) -> Result<Vec<Point>, StorageError> {
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{collection_name}' does not exist"))
        })?;

        collection.get_points(ids).await.map_err(|e| {
            StorageError::ServiceError(format!(
                "Failed to retrieve points from collection '{collection_name}': {e}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shard_routing() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let shard_holder = ShardHolder::new(HashMap::from_iter([
            (0, LocalShard::init(tmp_dir.path().join("0"), 0)),
            (1, LocalShard::init(tmp_dir.path().join("1"), 1)),
        ]));

        let shards_to_point_ids = shard_holder
            .select_shards(&[
                PointId::Id(1),
                PointId::Id(2),
                PointId::Id(100),
                PointId::Uuid("dummy-uuid".to_string()),
            ])
            .unwrap();

        let expected_grouping = HashMap::from_iter([
            (
                0,
                vec![
                    PointId::Id(1),
                    PointId::Id(100),
                    PointId::Uuid("dummy-uuid".to_string()),
                ],
            ),
            (1, vec![PointId::Id(2)]),
        ]);

        assert_eq!(shards_to_point_ids, expected_grouping);
    }
}
