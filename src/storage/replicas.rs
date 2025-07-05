use crate::{
    api::grpc::smoldb_p2p_grpc::{
        points_internal_client::PointsInternalClient, GetPointsRequest, UpsertPointsInternal,
    },
    channel_service::ChannelService,
    consensus::PeerId,
    storage::{
        content_manager::CollectionName,
        error::{CollectionError, CollectionResult, StorageError},
        segment::{Point, PointId},
        shard::{LocalShard, ShardId},
        shard_trait::{ShardOperationTrait, UpdateResult},
    },
};
use futures::future::BoxFuture;
use std::{collections::HashMap, future::Future, str::FromStr, sync::Arc};
use tokio::sync::RwLock;
use tonic::{async_trait, transport::Channel, Request, Status};

pub struct RemoteShard {
    pub id: ShardId,
    pub collection: CollectionName,
    pub peer_id: PeerId,
}

impl RemoteShard {
    /// Init a remote shard in memory that can be used to communicate with replicas on a remote peer.
    pub fn new(id: ShardId, collection: CollectionName, peer_id: PeerId) -> Self {
        RemoteShard {
            id,
            collection,
            peer_id,
        }
    }

    async fn current_address(
        &self,
        channel_service: &ChannelService,
    ) -> CollectionResult<http::Uri> {
        let guard_peer_addresses = channel_service.id_to_address.read().await;
        let peer_address = guard_peer_addresses.get(&self.peer_id).cloned();

        println!(
            "Remote shard {}:{} has address {:?}",
            self.peer_id, self.id, peer_address
        );

        match peer_address {
            Some(uri) => Ok(uri),
            None => Err(CollectionError::ServiceError(format!(
                "Peer {} does not have an address in the channel service",
                self.peer_id
            ))),
        }
    }

    async fn with_points_client<T, O: Future<Output = Result<T, Status>>>(
        &self,
        channel_service: ChannelService,
        f: impl Fn(PointsInternalClient<Channel>) -> O,
    ) -> CollectionResult<T> {
        let uri = self.current_address(&channel_service).await?;

        let channel = channel_service
            .channel_pool
            .get_or_create_channel(uri)
            .await?;

        let points_channel: PointsInternalClient<Channel> = PointsInternalClient::new(channel);

        f(points_channel).await.map_err(|e| {
            CollectionError::ServiceError(format!(
                "Failed to execute operation on remote shard {}: {}",
                self.id, e
            ))
        })
    }

    pub async fn upsert(&self, channel_service: ChannelService) -> Result<(), CollectionError> {
        let uri = self.current_address(&channel_service).await?;

        let channel = channel_service
            .channel_pool
            .get_or_create_channel(uri)
            .await?;

        let mut points_channel = PointsInternalClient::new(channel);

        let res = points_channel
            .upsert(tonic::Request::new(UpsertPointsInternal {
                shard_id: Some(self.id),
            }))
            .await?
            .into_inner();

        println!("Response from remote shard {}: {:?}", self.id, res);

        Ok(())
    }
}

#[async_trait]
impl ShardOperationTrait for RemoteShard {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>> {
        // Placeholder for actual remote shard logic
        // Err(CollectionError::ServiceError(
        //     "Remote shard get_points not implemented".to_string(),
        // ))

        let mut channel_service = ChannelService::default(); // Replace with actual channel service instance

        // let current_node_uri = http::Uri::from_str("http://localhost:50051").unwrap();

        let inner_map: HashMap<_, _> = HashMap::from_iter(vec![
            // (self.peer_id, current_node_uri),
            (101, http::Uri::from_str("http://0.0.0.0:5001").unwrap()),
            (102, http::Uri::from_str("http://0.0.0.0:5002").unwrap()),
            (103, http::Uri::from_str("http://0.0.0.0:5003").unwrap()),
        ]);
        channel_service.id_to_address = Arc::new(RwLock::new(inner_map));

        let return_all = ids.is_none();
        let ids = ids.unwrap_or_default();

        let ids = ids
            .into_iter()
            .filter_map(|id| {
                if let PointId::Id(id) = id {
                    Some(id)
                } else {
                    None // Skip non-ID point IDs
                }
            })
            .collect::<Vec<_>>();

        let get_points_response = self
            .with_points_client(channel_service, |mut client| {
                println!(
                    "Calling PointsInternalClient::get_points on remote shard {}:{}",
                    self.peer_id, self.id
                );
                let ids = ids.clone();
                async move {
                    let request = Request::new(GetPointsRequest {
                        collection_name: self.collection.clone(),
                        ids,
                        return_all,
                        shard_id: Some(self.id), // Ask the other node to return points only for this shard
                    });

                    client.get_points(request).await
                }
            })
            .await?
            .into_inner();

        let points = get_points_response
            .points
            .into_iter()
            .map(|p| Point {
                id: PointId::Id(p.id),
                payload: serde_json::from_str(&p.payload).unwrap(),
            })
            .collect::<Vec<_>>();

        Ok(points)
    }

    async fn update(&self, _wait: bool) -> CollectionResult<UpdateResult> {
        Ok(UpdateResult {
            // Placeholder for actual operation ID logic
            operation_id: Some(0_u64),
        })
    }
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

    pub async fn execute_cluster_read_operation<Res, F>(
        &self,
        read_operation: F,
        local_only: bool,
    ) -> CollectionResult<Vec<Res>>
    where
        F: Fn(&(dyn ShardOperationTrait + Send + Sync)) -> BoxFuture<'_, CollectionResult<Res>>,
    {
        let local_result = read_operation(&self.local).await?;
        let mut final_results = vec![local_result];

        if local_only {
            return Ok(final_results);
        }

        for remote in &self.remotes {
            let operation_result = read_operation(remote).await;
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
