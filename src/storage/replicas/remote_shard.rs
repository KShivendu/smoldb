use crate::{
    api::grpc::p2p_grpc_schema::{
        points_internal_client::PointsInternalClient, GetPointsRequest, Point as PointGrpc,
        UpsertPointsRequest,
    },
    channel_service::ChannelService,
    storage::{
        collection::CollectionName,
        error::{CollectionError, CollectionResult},
        replicas::ShardOperationTrait,
        segment::{Point, PointId},
    },
    types::{PeerId, ShardId},
};
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

    pub fn get_channel_service(&self) -> ChannelService {
        let mut channel_service = ChannelService::default();

        let inner_map: HashMap<_, _> = HashMap::from_iter(vec![
            (101, http::Uri::from_str("http://0.0.0.0:5001").unwrap()),
            (102, http::Uri::from_str("http://0.0.0.0:5002").unwrap()),
            (103, http::Uri::from_str("http://0.0.0.0:5003").unwrap()),
        ]);
        channel_service.id_to_address = Arc::new(RwLock::new(inner_map));

        channel_service
    }
}

#[async_trait]
impl ShardOperationTrait for RemoteShard {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>> {
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

        let channel_service = self.get_channel_service();

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

    async fn upsert_points(&self, points: Vec<Point>) -> CollectionResult<()> {
        let channel_service = self.get_channel_service();

        let _upsert_points_response = self
            .with_points_client(channel_service, |mut client| {
                let points = points.clone();
                async move {
                    client
                        .upsert_points(Request::new(UpsertPointsRequest {
                            collection_name: self.collection.clone(),
                            shard_id: None,
                            points: points
                                .into_iter()
                                .filter_map(|p| {
                                    if let PointId::Id(p_id) = p.id {
                                        // Only include points with PointId::Id
                                        Some(PointGrpc {
                                            id: p_id,
                                            payload: p.payload.to_string(),
                                        })
                                    } else {
                                        None // Skip UUIDs for now
                                    }
                                })
                                .collect(),
                        }))
                        .await
                }
            })
            .await?
            .into_inner();

        Ok(()) // Placeholder for actual remote shard logic
    }
}
