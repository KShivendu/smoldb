use crate::{
    api::{
        grpc::schema::{
            points_internal_client::PointsInternalClient, GetPointsRequest, Point as PointGrpc,
            QueryPointsRequest, UpsertPointsRequest,
        },
        points::Query,
    },
    channel_service::ChannelService,
    error::{CollectionError, CollectionResult},
    storage::{
        collection::CollectionName,
        replicas::{ShardOperationTrait, ShardState},
        segment::{Point, PointId},
    },
    types::{PeerId, ShardId},
};
use std::{future::Future, sync::Arc};
use tonic::{async_trait, transport::Channel, Request, Status};

pub struct RemoteShard {
    pub id: ShardId,
    pub collection: CollectionName,
    pub peer_id: PeerId,
    pub channel_service: Arc<ChannelService>,
    pub state: ShardState,
}

impl RemoteShard {
    /// Init a remote shard in memory that can be used to communicate with replicas on a remote peer.
    pub fn new(
        id: ShardId,
        collection: CollectionName,
        peer_id: PeerId,
        channel_service: Arc<ChannelService>,
    ) -> Self {
        RemoteShard {
            id,
            collection,
            peer_id,
            channel_service,
            state: ShardState::Active,
        }
    }

    async fn with_points_client<T, O: Future<Output = Result<T, Status>>>(
        &self,
        f: impl Fn(PointsInternalClient<Channel>) -> O,
    ) -> CollectionResult<T> {
        let uri = self.channel_service.get_uri(self.peer_id).await?;
        let channel = self.channel_service.get_or_create_channel(uri).await?;

        let points_channel: PointsInternalClient<Channel> = PointsInternalClient::new(channel);

        f(points_channel).await.map_err(|e| {
            CollectionError::ServiceError(format!(
                "Failed to execute operation on remote shard {}: {}",
                self.id, e
            ))
        })
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

        let get_points_response = self
            .with_points_client(|mut client| {
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
        let _upsert_points_response = self
            .with_points_client(|mut client| {
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

    async fn query_points(&self, query: Query) -> CollectionResult<Vec<Point>> {
        let query_response = self
            .with_points_client(|mut client| {
                let query = query.clone().into_grpc();
                async move {
                    // Placeholder for actual query logic
                    // This should be replaced with the actual query implementation
                    client
                        .query_points(Request::new(QueryPointsRequest {
                            collection_name: self.collection.clone(),
                            query,
                        }))
                        .await
                }
            })
            .await?
            .into_inner();

        let points = query_response
            .points
            .into_iter()
            .map(|p| Point {
                id: PointId::Id(p.id),
                payload: serde_json::from_str(&p.payload).unwrap(),
            })
            .collect::<Vec<_>>();

        Ok(points)
    }
}
