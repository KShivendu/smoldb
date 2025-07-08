use crate::{
    api::points::PointsOperation,
    channel_service::ChannelService,
    storage::{
        error::{CollectionResult, StorageError},
        replicas::{ReplicaHolder, ReplicaSet},
        segment::{Point, PointId},
        shard::{LocalShard, ShardId},
        shard_trait::ShardOperationTrait,
    },
};
use futures::FutureExt;
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
    pub channel_service: ChannelService,
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

pub type CollectionName = String;

pub struct Collection {
    pub id: CollectionName,
    pub config: CollectionConfig,
    pub replica_holder: Arc<RwLock<ReplicaHolder>>,
    pub path: PathBuf,
}

impl Collection {
    pub async fn init(
        id: CollectionName,
        config: CollectionConfig,
        path: &Path,
    ) -> Result<Self, StorageError> {
        // ToDo: Create ShardHolder & ShardReplicaSet

        config.save(path)?;

        // ToDo: Initialize shards == num_cpus for max parallelism
        let s0_path = path.join("0");
        let s1_path = path.join("1");

        let shards = [s0_path, s1_path]
            .into_iter()
            .enumerate()
            .map(|(shard_id, shard_path)| {
                let shard_id = shard_id as ShardId;
                let replica_set = ReplicaSet::new(
                    LocalShard::init(shard_path, shard_id),
                    vec![], // No remote shards for now
                    id.clone(),
                );

                Ok((shard_id, replica_set))
            })
            .collect::<Result<HashMap<_, _>, StorageError>>()?;

        Ok(Collection {
            id,
            config,
            replica_holder: Arc::new(RwLock::new(ReplicaHolder::new(shards))),
            path: path.to_owned(),
        })
    }

    pub fn load(id: CollectionName, path: &Path) -> Result<Self, StorageError> {
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
        let mut replicas = HashMap::new();
        let dir_contents = std::fs::read_dir(path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to read collection directory: {e}"))
        })?;

        for entry in dir_contents {
            let path = entry.expect("Can't read directory entry").path();
            if !path.is_dir() {
                continue; // Skip non-directory entries
            }

            let shard = LocalShard::load(&path)?;
            let shard_id = shard.id;

            replicas.insert(
                shard_id,
                // ToDo: Load remote shards if any
                ReplicaSet::new(shard, vec![], id.clone()),
            );
        }

        Ok(Collection {
            id,
            config,
            replica_holder: Arc::new(RwLock::new(ReplicaHolder::new(replicas))),
            path: path.to_path_buf(),
        })
    }

    pub async fn upsert_points(&self, points: &[Point], local_only: bool) -> CollectionResult<()> {
        let shard_holder = &self.replica_holder.read().await;

        let points_map: HashMap<PointId, Point> = points
            .iter()
            .map(|point| (point.id.clone(), point.clone()))
            .collect();

        let point_ids: Vec<_> = points.iter().map(|point| point.id.clone()).collect();

        // ToDo: Run this operation concurrently for each shard
        for (shard_id, shard_point_ids) in shard_holder.select_shards(&point_ids)? {
            let replica_set = shard_holder
                .shards
                .get(&shard_id)
                .ok_or_else(|| StorageError::BadInput(format!("Shard {shard_id} not found")))?;

            let points = shard_point_ids
                .iter()
                .filter_map(|id| points_map.get(id).cloned())
                .collect::<Vec<_>>();

            let _ = replica_set
                .execute_cluster_read_operation(
                    |shard| {
                        let points_cloned = points.clone();
                        async move { shard.upsert_points(points_cloned).await }.boxed()
                    },
                    local_only,
                )
                .await?;

            // replica_set.local.insert_points(&points)?
        }

        Ok(())
    }

    pub async fn get_points(
        &self,
        ids: Option<Vec<PointId>>,
        shard_id: Option<ShardId>,
        local_only: bool,
    ) -> CollectionResult<Vec<Point>> {
        let replica_holder = self.replica_holder.read().await;

        let Some(ids) = ids else {
            // If no ids are provided, return all points from all shards
            let mut all_points = vec![];
            for (current_shard_id, replica_set) in replica_holder.shards.iter() {
                if let Some(desired_shard) = shard_id {
                    if *current_shard_id != desired_shard {
                        continue; // Skip shards that are not the desired shard
                    }
                }

                let replica_results = replica_set
                    .execute_cluster_read_operation(
                        |shard| {
                            let ids_cloned = ids.clone();
                            async move { shard.get_points(ids_cloned).await }.boxed()
                        },
                        local_only,
                    )
                    .await?;

                all_points.extend(replica_results.into_iter().flatten());
            }
            return Ok(all_points);
        };

        if let Some(shard_id) = shard_id {
            let replica_set = replica_holder.get_replica_set(shard_id).await?;
            Ok(replica_set.local.get_points(Some(ids)).await?)
        } else {
            let mut points = vec![];

            for (shard_id, shard_point_ids) in replica_holder.select_shards(&ids)? {
                let replica_set = replica_holder.get_replica_set(shard_id).await?;

                let collected_points = replica_set.local.get_points(Some(shard_point_ids)).await?;
                points.extend(collected_points);
            }

            Ok(points)
        }
    }
}

#[derive(Serialize)]
pub struct CollectionInfo {
    pub id: CollectionName,
    pub config: CollectionConfig,
    pub shard_count: usize,
    pub segment_count: usize,
}

impl CollectionInfo {
    pub async fn from(collection: &Collection) -> Self {
        let shard_holder = collection.replica_holder.read().await;
        CollectionInfo {
            id: collection.id.clone(),
            config: collection.config.clone(),
            shard_count: shard_holder.shards.len(),
            segment_count: shard_holder
                .shards
                .values()
                .map(|shard| shard.local.segments.len())
                .sum(),
        }
    }
}

pub type Collections = HashMap<CollectionName, Collection>;

pub enum CollectionMetaOperation {
    CreateCollection {
        collection_name: String,
        params: String,
    },
}

impl TableOfContent {
    pub fn load(channel_service: ChannelService) -> Self {
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
            channel_service,
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
                    Collection::init(collection_name.clone(), CollectionConfig { params }, &path)
                        .await?;

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
                    .upsert_points(&upsert_points.points, false)
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
        ids: Option<Vec<PointId>>,
    ) -> Result<Vec<Point>, StorageError> {
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{collection_name}' does not exist"))
        })?;

        collection.get_points(ids, None, false).await.map_err(|e| {
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

        let s0 = LocalShard::init(tmp_dir.path().join("0"), 0);
        let s1 = LocalShard::init(tmp_dir.path().join("1"), 1);

        let shard_holder = ReplicaHolder::new(HashMap::from_iter([
            (0, ReplicaSet::new(s0, vec![], "c1".to_string())),
            (1, ReplicaSet::new(s1, vec![], "c1".to_string())),
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
