use crate::api::grpc::schema::QueryPointsParams as GrpcQueryPointsParams;
use crate::api::points::Query;
use crate::storage::index::filter::QueryFilter;

impl Query {
    pub fn from_grpc(query: Option<GrpcQueryPointsParams>) -> Result<Self, String> {
        let query = query.ok_or_else(|| "Query parameters are required".to_string())?;

        let filter = query
            .filter
            .ok_or_else(|| "Query filter is required".to_string())?;

        Ok(Self {
            filter: QueryFilter {
                key: filter.key,
                value: filter.value,
                op: filter.op.into(),
            },
        })
    }

    pub fn into_grpc(self) -> Option<GrpcQueryPointsParams> {
        Some(GrpcQueryPointsParams {
            filter: Some(crate::api::grpc::schema::QueryFilter {
                key: self.filter.key,
                value: self.filter.value,
                op: self.filter.op.as_str().to_string(),
            }),
        })
    }
}
