use crate::{
    storage::{
        error::{CollectionError, CollectionResult, StorageError},
        replicas::{local_shard::LocalShard, ReplicaHolder, ReplicaSet, ShardOperationTrait},
        segment::{Point, PointId},
    },
    types::ShardId,
};
use futures::FutureExt;
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

        // ToDo: Add remote shards to replica holder while creating a new collection?

        Ok(Collection {
            id,
            config,
            replica_holder: Arc::new(RwLock::new(ReplicaHolder::new(shards))),
            path: path.to_owned(),
        })
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

    /// Upserts points into the collection.
    ///
    /// This is not cancel safe at the moment.
    pub async fn upsert_points(
        &self,
        points: Vec<Point>,
        local_only: bool,
    ) -> CollectionResult<()> {
        let shard_holder = &self.replica_holder.read().await;

        let point_ids: Vec<_> = points.iter().map(|point| point.id.clone()).collect();

        let points_map: HashMap<PointId, Point> = points
            .into_iter()
            .map(|point| (point.id.clone(), point))
            .collect();

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

            let results = replica_set
                .execute_cluster_operation(
                    |shard| {
                        let points_cloned = points.clone();
                        async move { shard.upsert_points(points_cloned).await }.boxed()
                    },
                    local_only,
                )
                .await;

            let total_success = results.iter().filter(|r| r.is_ok()).count();

            let (local_result, _remote_results) = results.split_first().unwrap();
            if let Err(e) = local_result {
                return Err(CollectionError::ServiceError(format!(
                    "Failed to upsert points in local shard {shard_id}: {e}"
                )));
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
