use std::{collections::HashMap, sync::Arc};

use tonic::{async_trait, Response};

use crate::{
    api::grpc::smoldb_p2p_grpc::{
        points_internal_server::PointsInternal, GetPointsRequest, GetPointsResponse, Point,
        PointsOperationResponseInternal, UpsertPointsInternal,
    },
    storage::{content_manager::TableOfContent, segment::PointId},
};

pub struct PointsInternalService {
    toc: Arc<TableOfContent>,
}

impl PointsInternalService {
    pub fn new(toc: Arc<TableOfContent>) -> Self {
        PointsInternalService { toc }
    }
}

#[async_trait]
impl PointsInternal for PointsInternalService {
    async fn get_points(
        &self,
        request: tonic::Request<GetPointsRequest>,
    ) -> Result<Response<GetPointsResponse>, tonic::Status> {
        let request: GetPointsRequest = request.into_inner();
        let collection_name = request.collection_name;

        println!(
            "Received internal request to get points from collection: {}",
            collection_name
        );

        let collections = self.toc.collections.read().await;
        let collection = collections.get(&collection_name).ok_or_else(|| {
            tonic::Status::not_found(format!("Collection '{}' not found", collection_name))
        })?;

        let point_ids = if request.return_all {
            None
        } else {
            Some(
                request
                    .ids
                    .into_iter()
                    .map(|id| PointId::Id(id))
                    .collect::<Vec<_>>(),
            )
        };

        let points = collection
            .get_points(point_ids, request.shard_id, true)
            .await;

        let points = points.map_err(|e| {
            tonic::Status::internal(format!(
                "Failed to retrieve points from collection '{}': {}",
                collection_name, e
            ))
        })?;

        Ok(Response::new(GetPointsResponse {
            points: points
                .into_iter()
                .filter_map(|p| {
                    if let PointId::Id(id) = p.id {
                        // FixMe: This is a workaround for not being able to pass json just yet.
                        let payload = p
                            .payload
                            .as_object()
                            .and_then(|hashmap| {
                                Some(
                                    hashmap
                                        .into_iter()
                                        .map(|(k, v)| (k.to_string(), v.to_string()))
                                        .collect::<HashMap<_, _>>(),
                                )
                            })
                            .unwrap();

                        return Some(Point {
                            id,
                            payload: payload,
                        });
                    }
                    None // ignore UUIDs for now
                })
                .collect(),
        }))
    }

    async fn upsert(
        &self,
        _request: tonic::Request<UpsertPointsInternal>,
    ) -> Result<Response<PointsOperationResponseInternal>, tonic::Status> {
        // This is a stub implementation. Replace with actual logic to upsert points.
        Ok(Response::new(PointsOperationResponseInternal {
            message: "Upsert operation completed".to_string(),
        }))
    }
}
