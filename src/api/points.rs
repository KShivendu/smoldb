use crate::{
    api::{dispatcher::Dispatcher, helpers},
    error::CollectionError,
    storage::{
        index::filter::QueryFilter,
        segment::{Point, PointId},
    },
};
use actix_web::{
    get, post, put,
    web::{self, Json},
    Responder,
};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct UpsertPoints {
    pub points: Vec<Point>,
}

#[derive(serde::Serialize, ToSchema)]
pub struct UpsertPointsResponse {
    pub num_points: usize,
}

#[utoipa::path(
    tag = "Points",
    responses(
        (status = 200, description = "Upsert points into a collection", body = UpsertPointsResponse),
    ),
)]
#[put("/collections/{collection_name}/points")]
async fn upsert_points(
    collection_name: web::Path<String>,
    operation: Json<UpsertPoints>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();
        let operation = operation.into_inner();

        let num_points = operation.points.len();

        let result = dispatcher
            .toc
            .upsert_points(&collection_name, operation.points)
            .await;

        match result {
            Ok(()) => Ok(UpsertPointsResponse { num_points }),
            Err(e) => Err(CollectionError::ServiceError(format!(
                "Error upserting points in collection '{collection_name}': {e}"
            ))),
        }
    })
    .await
}

#[derive(serde::Serialize, ToSchema)]
pub struct GetPointResponse {
    pub point: Point,
}

#[utoipa::path(
    tag = "Points",
    responses(
        (status = 200, description = "Get a point from a collection", body = GetPointResponse),
    ),
)]
#[get("/collections/{collection_name}/points/{id}")]
pub async fn get_point(
    path: web::Path<(String, String)>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let (collection_name, id) = path.into_inner();

        let point_id = match id.parse::<u64>() {
            Ok(id) => PointId::Id(id),
            Err(_) => PointId::try_from(id.as_str())?,
        };

        let result = dispatcher
            .toc
            .read_points(&collection_name, Some(vec![point_id]))
            .await;

        match result {
            Ok(points) if points.is_empty() => Err(CollectionError::ServiceError(format!(
                "Point with id '{id}' not found in collection '{collection_name}'"
            ))),
            Ok(points) => Ok(GetPointResponse {
                point: points[0].clone(),
            }),
            Err(e) => Err(CollectionError::ServiceError(format!(
                "Error retrieving point with id '{id}' in collection '{collection_name}': {e}"
            ))),
        }
    })
    .await
}

#[derive(serde::Serialize, ToSchema)]
pub struct ListPointsResponse {
    pub points: Vec<Point>,
}

#[utoipa::path(
    tag = "Points",
    responses(
        (status = 200, description = "List all points in a collection", body = ListPointsResponse),
    ),
)]
#[get("/collections/{collection_name}/points")]
pub async fn list_points(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name: String = collection_name.into_inner();
        let result = dispatcher.toc.read_points(&collection_name, None).await;
        match result {
            Ok(points) => {
                if points.is_empty() {
                    Err(CollectionError::ServiceError(format!(
                        "No points found in collection '{collection_name}'"
                    )))
                } else {
                    Ok(ListPointsResponse { points })
                }
            }
            Err(e) => Err(CollectionError::ServiceError(format!(
                "Error listing points in collection '{collection_name}': {e}"
            ))),
        }
    })
    .await
}

#[derive(Deserialize, Clone)]
pub struct Query {
    pub filter: QueryFilter,
    /// Warning: If not set, latency will be very high
    pub limit: Option<usize>,
}

#[utoipa::path(
    tag = "Points",
    responses(
        (status = 200, description = "Query points in a collection", body = ListPointsResponse),
    ),
)]
#[post("/collections/{collection_name}/query")]
pub async fn query_points(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
    query: Json<Query>,
) -> impl Responder {
    helpers::time(async {
        let collection_name: String = collection_name.into_inner();
        let mut query = query.into_inner();

        // Set default limit if not provided
        if query.limit.is_none() {
            query.limit = Some(10); // Default limit
        }

        let result = dispatcher.toc.query_points(&collection_name, query).await;
        match result {
            Ok(points) => Ok(ListPointsResponse { points }),
            Err(e) => Err(CollectionError::ServiceError(format!(
                "Error listing points in collection '{collection_name}': {e}"
            ))),
        }
    })
    .await
}
