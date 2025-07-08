use tonic::async_trait;

use crate::storage::{
    error::CollectionResult,
    segment::{Point, PointId},
};

#[derive(Copy, Clone, Debug)]
pub struct UpdateResult {
    pub operation_id: Option<u64>,
}

#[async_trait]
pub trait ShardOperationTrait {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>>;
    async fn upsert_points(&self, points: Vec<Point>) -> CollectionResult<()>;
}
