use crate::{
    api::grpc::smoldb_p2p_grpc::{
        points_internal_server::PointsInternal, GetPointsRequest, GetPointsResponse,
        Point as GrpcPoint, UpsertPointsRequest, UpsertPointsResponse,
    },
    storage::{
        content_manager::TableOfContent,
        segment::{Point, PointId},
    },
};
use std::sync::Arc;
use tonic::{async_trait, Response};

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
        let GetPointsRequest {
            collection_name,
            ids,
            return_all,
            shard_id,
        } = request.into_inner();

        println!("Received internal request to get points from collection: {collection_name}");

        let collections = self.toc.collections.read().await;
        let collection = collections.get(&collection_name).ok_or_else(|| {
            tonic::Status::not_found(format!("Collection '{collection_name}' not found"))
        })?;

        let point_ids = if return_all {
            None
        } else {
            Some(ids.into_iter().map(PointId::Id).collect::<Vec<_>>())
        };

        let points = collection.get_points(point_ids, shard_id, true).await;

        let points = points.map_err(|e| {
            tonic::Status::internal(format!(
                "Failed to retrieve points from collection '{collection_name}': {e}"
            ))
        })?;

        Ok(Response::new(GetPointsResponse {
            points: points
                .into_iter()
                .filter_map(|p| {
                    if let PointId::Id(id) = p.id {
                        // FixMe: This is a workaround for not being able to pass json just yet.
                        let payload = p.payload.to_string();
                        return Some(GrpcPoint { id, payload });
                    }
                    None // ignore UUIDs for now
                })
                .collect(),
        }))
    }

    async fn upsert_points(
        &self,
        _request: tonic::Request<UpsertPointsRequest>,
    ) -> Result<Response<UpsertPointsResponse>, tonic::Status> {
        let UpsertPointsRequest {
            collection_name,
            points,
            shard_id: _, // ToDo: We should specify shard_id when upserting?
        } = _request.into_inner();
        println!("Received internal request to upsert points from collection: {collection_name}");

        let collections = self.toc.collections.read().await;
        let collection = collections.get(&collection_name).ok_or_else(|| {
            tonic::Status::not_found(format!("Collection '{collection_name}' not found"))
        })?;

        let points = points
            .into_iter()
            .map(|p| Point {
                id: PointId::Id(p.id),
                payload: serde_json::from_str(&p.payload).unwrap(),
            })
            .collect::<Vec<_>>();

        collection.upsert_points(&points, true).await.map_err(|e| {
            tonic::Status::internal(format!(
                "Failed to upsert points in collection '{collection_name}': {e}"
            ))
        })?;

        Ok(Response::new(UpsertPointsResponse {
            message: "Upsert operation completed".to_string(),
        }))
    }
}
