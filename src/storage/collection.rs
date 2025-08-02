use crate::{
    api::points::Query,
    channel_service::ChannelService,
    error::{CollectionError, CollectionResult, StorageError},
    storage::{
        index::payload_index::IndexConfig,
        replicas::{
            local_shard::LocalShard, ReplicaHolder, ReplicaSet, ShardOperationTrait, ShardState,
        },
        segment::{Point, PointId},
    },
    types::{PeerId, ShardId},
};
use futures::FutureExt;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

pub const COLLECTION_CONFIG_FILE: &str = "config.json";

pub const DEFAULT_CONSISTENCY_FACTOR: usize = 2;

pub type CollectionName = String;

pub struct Collection {
    pub id: CollectionName,
    pub config: CollectionConfig,
    pub replica_holder: Arc<RwLock<ReplicaHolder>>,
    pub path: PathBuf,
    pub channel_service: Arc<ChannelService>,
}

impl Collection {
    pub async fn init(
        id: CollectionName,
        config: CollectionConfig,
        path: &Path,
        channel_service: Arc<ChannelService>,
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
                // No remote replicas initially. They will be added by Collection::add_remote_replicas
                let replica_set = ReplicaSet::new(
                    LocalShard::init(shard_path, shard_id, Some(config.payload_schema.clone())),
                    id.clone(),
                    shard_id,
                    channel_service.clone(),
                );

                Ok((shard_id, replica_set))
            })
            .collect::<Result<HashMap<_, _>, StorageError>>()?;

        Ok(Collection {
            id,
            config,
            replica_holder: Arc::new(RwLock::new(ReplicaHolder::new(shards))),
            path: path.to_owned(),
            channel_service,
        })
    }

    /// ToDo: Should only add a remote replica, but only for one shard at a time
    pub async fn add_remote_replicas(&self, peer_id: PeerId) -> Result<(), StorageError> {
        let mut replica_holder = self.replica_holder.write().await;
        for (_shard_id, replica_set) in replica_holder.shards.iter_mut() {
            replica_set.add_remote(peer_id).await;
        }

        Ok(())
    }

    pub fn delete(&self) -> Result<(), StorageError> {
        let collection_path = self.path.clone();
        if collection_path.exists() {
            std::fs::remove_dir_all(&collection_path).map_err(|e| {
                StorageError::ServiceError(format!("Failed to delete collection: {e}"))
            })?;
        } else {
            return Err(StorageError::BadInput(
                "Collection directory does not exist".to_string(),
            ));
        }

        Ok(())
    }

    pub fn load(
        id: CollectionName,
        path: &Path,
        channel_service: Arc<ChannelService>,
    ) -> Result<Self, StorageError> {
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
                ReplicaSet::new(shard, id.clone(), shard_id, channel_service.clone()),
            );
        }

        Ok(Collection {
            id,
            config,
            replica_holder: Arc::new(RwLock::new(ReplicaHolder::new(replicas))),
            path: path.to_path_buf(),
            channel_service,
        })
    }

    /// Upserts points into the collection.
    ///
    /// This is not cancel safe at the moment.
    pub async fn upsert_points(
        &self,
        points: Vec<Point>,
        local_only: bool,
    ) -> CollectionResult<()> {
        let replica_holder_guard = self.replica_holder.read().await;

        let point_ids: Vec<_> = points.iter().map(|point| point.id.clone()).collect();

        let points_map: HashMap<PointId, Point> = points
            .into_iter()
            .map(|point| (point.id.clone(), point))
            .collect();

        let mut replicas_to_mark_dead = vec![];

        // ToDo: Run this operation concurrently for each shard
        for (shard_id, shard_point_ids) in replica_holder_guard.select_shards(&point_ids)? {
            let replica_set = replica_holder_guard
                .shards
                .get(&shard_id)
                .ok_or_else(|| StorageError::BadInput(format!("Shard {shard_id} not found")))?;

            let points = shard_point_ids
                .iter()
                .filter_map(|id| points_map.get(id).cloned())
                .collect::<Vec<_>>();

            let results = replica_set
                .execute_cluster_operation(
                    |shard| {
                        let points_cloned = points.clone();
                        async move { shard.upsert_points(points_cloned).await }.boxed()
                    },
                    local_only,
                )
                .await;

            let total_success = results.iter().filter(|(_, r)| r.is_ok()).count();

            let ((_peer_id, local_result), remote_results) = results.split_first().unwrap();
            if let Err(e) = local_result {
                return Err(CollectionError::ServiceError(format!(
                    "Failed to upsert points in local shard {shard_id}: {e}"
                )));
            }

            for (remote_peer_id, result) in remote_results {
                if let Err(e) = result {
                    error!("Failed to upsert points in remote shard {shard_id} for peer {remote_peer_id}: {e}. Will mark it as dead.");
                    replicas_to_mark_dead.push((shard_id, *remote_peer_id));
                }
            }

            // ToDo: Both should have collection level config
            let write_consistency_factor = DEFAULT_CONSISTENCY_FACTOR;
            let num_replicas = replica_set.num_replicas();

            let min_desired_success = if local_only {
                1
            } else {
                num_replicas.min(write_consistency_factor)
            };

            if total_success < min_desired_success {
                return Err(CollectionError::ServiceError(format!(
                    "Failed to upsert points in shard {shard_id}: only {total_success} out of {num_replicas} replicas succeeded"
                )));
            }
        }

        // Release the read lock before taking write lock
        drop(replica_holder_guard);

        // Mark the replicas that missed the update as Dead
        for (shard_id, peer_id) in replicas_to_mark_dead {
            self.replica_holder
                .write()
                .await
                .set_replica_state(shard_id, peer_id, None, ShardState::Dead)
                .await?;
            info!(
                "Marked remote shard {shard_id} for peer {peer_id} as Dead due to failed upsert."
            );
        }

        Ok(())
    }

    pub async fn read_points(
        &self,
        ids: Option<Vec<PointId>>,
        shard_id: Option<ShardId>,
        local_only: bool,
    ) -> CollectionResult<Vec<Point>> {
        let replica_holder = self.replica_holder.read().await;

        let Some(ids) = ids else {
            // If no ids are provided, return all points from all shards
            let mut all_points = BTreeMap::new();
            for (current_shard_id, replica_set) in replica_holder.shards.iter() {
                if let Some(desired_shard) = shard_id {
                    if *current_shard_id != desired_shard {
                        continue; // Skip shards that are not the desired shard
                    }
                }

                let replica_results = replica_set
                    .execute_cluster_operation(
                        |shard| {
                            let ids_cloned = ids.clone();
                            async move { shard.get_points(ids_cloned).await }.boxed()
                        },
                        local_only,
                    )
                    .await
                    .into_iter()
                    .map(|(_peer_id, r)| r)
                    .collect::<Result<Vec<_>, _>>()?;

                for point in replica_results.into_iter().flatten() {
                    // We only insert if the point is not already present so that local shard takes precedence
                    if let Entry::Vacant(e) = all_points.entry(point.id.clone()) {
                        e.insert(point);
                    }
                }
            }
            return Ok(all_points.into_values().collect());
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

    pub async fn query_points(&self, query: Query) -> CollectionResult<Vec<Point>> {
        let replica_holder = self.replica_holder.read().await;

        let mut results = vec![];
        for (_shard_id, replica_set) in replica_holder.shards.iter() {
            let shard_results = replica_set.local.query_points(query.clone()).await?;
            results.extend(shard_results);
        }

        // ToDo: Implement cluster level query

        Ok(results)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectionConfig {
    pub params: String,
    pub payload_schema: BTreeMap<String, IndexConfig>,
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

#[cfg(test)]
mod tests {
    use crate::storage::index::filter::{FilterOperator, QueryFilter};

    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_collection_init() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let path = temp_dir.path();
        let collection_name = "test_collection".to_string();
        let config = CollectionConfig {
            params: "test_params".to_string(),
            payload_schema: BTreeMap::new(),
        };

        let collection = Collection::init(
            collection_name,
            config,
            path,
            Arc::new(ChannelService::empty(100)),
        )
        .await
        .expect("Failed to initialize collection");

        let config_path = path.join("config.json");

        assert_eq!(collection.id, "test_collection");
        assert!(collection.path.exists());
        assert!(config_path.exists());

        let replica_holder = collection.replica_holder.read().await;
        assert_eq!(replica_holder.shards.len(), 2);
        assert_eq!(replica_holder.shards[&0].remotes.len(), 0);
        assert_eq!(replica_holder.shards[&1].remotes.len(), 0);

        let shard0_dir = path.join("0");
        let shard1_dir = path.join("1");
        assert!(shard0_dir.exists());
        assert!(shard1_dir.exists());
    }

    #[test]
    fn test_collection_config_save() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let path = temp_dir.path();
        let config = CollectionConfig {
            params: "test_params".to_string(),
            payload_schema: BTreeMap::new(),
        };

        config.save(path).expect("Failed to save collection config");

        let config_path = path.join(COLLECTION_CONFIG_FILE);
        assert!(config_path.exists());
        let loaded_config: CollectionConfig = serde_json::from_reader(
            std::fs::File::open(config_path).expect("Failed to open config file"),
        )
        .expect("Failed to parse collection config JSON");

        assert_eq!(loaded_config.params, config.params);
        assert_eq!(loaded_config.payload_schema, config.payload_schema);
    }

    #[tokio::test]
    async fn test_collection_load() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let path = temp_dir.path();
        let collection_name = "test_collection".to_string();
        let config = CollectionConfig {
            params: "test_params".to_string(),
            payload_schema: BTreeMap::from_iter([
                ("field1".to_string(), IndexConfig::Int),
                ("field2".to_string(), IndexConfig::Null),
            ]),
        };

        config.save(path).expect("Failed to save collection config");

        let collection = Collection::load(
            collection_name.clone(),
            path,
            Arc::new(ChannelService::empty(100)),
        )
        .expect("Failed to load collection");

        assert_eq!(collection.id, collection_name);
        assert_eq!(collection.config.params, config.params);
        assert_eq!(collection.config.payload_schema, config.payload_schema);
    }

    #[tokio::test]
    async fn test_collection_write_read_points() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let path = temp_dir.path();
        let collection_name = "test_collection".to_string();

        let config = CollectionConfig {
            params: "test_params".to_string(),
            payload_schema: BTreeMap::from_iter([("age".to_string(), IndexConfig::Int)]),
        };

        let collection = Collection::init(
            collection_name,
            config,
            path,
            Arc::new(ChannelService::empty(100)),
        )
        .await
        .expect("Failed to initialize collection");

        let points = vec![
            Point {
                id: PointId::Id(1),
                payload: json!({"field1": "value1", "age": 10}),
            },
            Point {
                id: PointId::Id(2),
                payload: json!({"field1": "value2", "age": 20}),
            },
            Point {
                id: PointId::Id(3),
                payload: json!({"field1": "value3", "age": 30}),
            },
        ];

        collection
            .upsert_points(points.clone(), false)
            .await
            .expect("Failed to upsert points");

        let read_all_points = collection
            .read_points(None, None, false)
            .await
            .expect("Failed to read all points");

        assert_eq!(read_all_points, points);

        let read_points = collection
            .read_points(Some(vec![PointId::Id(1)]), None, false)
            .await
            .expect("Failed to read points");

        assert_eq!(read_points.len(), 1);
        assert_eq!(read_points[0], points[0]);

        let query = Query {
            filter: QueryFilter::new("age", "25", FilterOperator::Gte),
        };

        let queried_points = collection
            .query_points(query)
            .await
            .expect("Failed to query points");

        assert_eq!(queried_points.len(), 1);
        assert_eq!(queried_points[0], points[2]);
    }
}
