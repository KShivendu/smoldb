use crate::api::grpc::schema::QueryPointsParams as GrpcQueryPointsParams;
use crate::api::points::Query;
use crate::storage::index::filter::QueryFilter;

impl Query {
    pub fn from_grpc(query: Option<GrpcQueryPointsParams>) -> Result<Self, String> {
        let GrpcQueryPointsParams { filter, limit } =
            query.ok_or_else(|| "Query parameters are required".to_string())?;

        let filter = filter.ok_or_else(|| "Query filter is required".to_string())?;

        let value = serde_json::from_str(&filter.value).expect("Failed to convert value to string");

        Ok(Self {
            filter: QueryFilter {
                key: filter.key,
                value,
                op: filter.op.try_into()?,
            },
            limit: limit.map(|l| l as usize),
        })
    }

    pub fn into_grpc(self) -> Option<GrpcQueryPointsParams> {
        // todo: cleaner error handling
        let value =
            serde_json::to_string(&self.filter.value).expect("Failed to convert value to string");

        Some(GrpcQueryPointsParams {
            filter: Some(crate::api::grpc::schema::QueryFilter {
                key: self.filter.key,
                value,
                op: self.filter.op.as_str().to_string(),
            }),
            limit: self.limit.map(|l| l as u64),
        })
    }
}
