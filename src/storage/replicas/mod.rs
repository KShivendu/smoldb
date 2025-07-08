pub mod local_shard;
pub mod remote_shard;

use crate::storage::replicas::local_shard::LocalShard;
use crate::storage::replicas::remote_shard::RemoteShard;
use crate::storage::segment::Point;
use crate::storage::{
    collection::CollectionName,
    error::{CollectionResult, StorageError},
    segment::PointId,
};
use crate::types::{PeerId, ShardId};
use futures::future::BoxFuture;
use std::collections::HashMap;
use tonic::async_trait;

#[derive(Copy, Clone, Debug)]
pub struct UpdateResult {
    pub operation_id: Option<u64>,
}

#[async_trait]
pub trait ShardOperationTrait {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>>;
    async fn upsert_points(&self, points: Vec<Point>) -> CollectionResult<()>;
}

pub struct ReplicaSet {
    pub local: LocalShard,
    pub remotes: Vec<RemoteShard>,

    #[allow(dead_code)]
    collection_id: CollectionName,
}

impl ReplicaSet {
    pub fn new(local: LocalShard, remotes: Vec<PeerId>, collection_id: CollectionName) -> Self {
        let remotes = remotes
            .into_iter()
            .map(|peer_id| RemoteShard::new(local.id, collection_id.clone(), peer_id))
            .collect();

        ReplicaSet {
            local,
            remotes,
            collection_id,
        }
    }

    /// Executes the operation on the local shard and then on all the remote shards.
    /// If `local_only` is true, it only executes on the local shard.
    pub async fn execute_cluster_operation<Res, F>(
        &self,
        operation: F,
        local_only: bool,
    ) -> CollectionResult<Vec<Res>>
    where
        F: Fn(&(dyn ShardOperationTrait + Send + Sync)) -> BoxFuture<'_, CollectionResult<Res>>,
    {
        let local_result = operation(&self.local).await?;
        let mut final_results = vec![local_result];

        if local_only {
            return Ok(final_results);
        }

        for remote in &self.remotes {
            let operation_result = operation(remote).await;
            match operation_result {
                Ok(res) => final_results.push(res),
                Err(e) => {
                    // Ignore errors from remote shards, but log them
                    println!(
                        "Error executing operation on remote shard {}/{}: {}",
                        remote.peer_id, remote.id, e
                    );
                }
            }
        }

        Ok(final_results)
    }
}

pub struct ReplicaHolder {
    pub shards: HashMap<ShardId, ReplicaSet>,
    ring: hashring::HashRing<ShardId>,
}

impl ReplicaHolder {
    pub fn new(shards: HashMap<ShardId, ReplicaSet>) -> Self {
        let mut ring = hashring::HashRing::new();
        for shard_id in shards.keys() {
            ring.add(*shard_id);
        }

        ReplicaHolder { shards, ring }
    }

    pub fn dummy() -> Self {
        ReplicaHolder {
            shards: HashMap::new(),
            ring: hashring::HashRing::new(),
        }
    }

    pub async fn get_replica_set(&self, shard_id: ShardId) -> Result<&ReplicaSet, StorageError> {
        let replica_set = self
            .shards
            .get(&shard_id)
            .ok_or_else(|| StorageError::BadInput(format!("Shard {shard_id} not found")))?;

        Ok(replica_set)
    }

    // Wrong abstraction: but add remote shards for a given collection in each of the shards.
    pub async fn add_remote_shards(
        &mut self,
        peer_id: PeerId,
        collection: CollectionName,
    ) -> Result<(), StorageError> {
        for (shard_id, replica_set) in self.shards.iter_mut() {
            // Add if not exists
            if replica_set.remotes.iter().any(|r| r.peer_id == peer_id) {
                continue; // Skip if remote shard already exists
            }

            replica_set
                .remotes
                .push(RemoteShard::new(*shard_id, collection.clone(), peer_id));
        }

        // ToDo: What happens to hashring if shard already exists when you add?
        // self.ring.add(shard_id);

        Ok(())
    }

    pub fn select_shards(
        &self,
        point_ids: &[PointId],
    ) -> Result<HashMap<ShardId, Vec<PointId>>, StorageError> {
        let mut shards_to_point_ids = HashMap::new();
        for point_id in point_ids {
            let shard_id = self
                .ring
                .get(&point_id)
                .ok_or_else(|| StorageError::ServiceError("No shards found".to_string()))?;

            shards_to_point_ids
                .entry(*shard_id)
                .or_insert_with(Vec::new)
                .push(point_id.clone());
        }
        Ok(shards_to_point_ids)
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
