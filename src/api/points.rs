use crate::{
    api::{collection::Dispatcher, helpers},
    storage::{
        error::CollectionError,
        segment::{Point, PointId},
    },
};
use actix_web::{
    web::{self, Json},
    Responder,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct UpsertPoints {
    pub points: Vec<Point>,
}

pub enum PointsOperation {
    Upsert(UpsertPoints),
}

#[derive(serde::Serialize)]
pub struct UpsertPointsResponse {
    pub num_points: usize,
}

#[actix_web::put("/collections/{collection_name}/points")]
async fn upsert_points(
    collection_name: web::Path<String>,
    operation: Json<UpsertPoints>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();
        let operation = operation.into_inner();

        let num_points = operation.points.len();

        // ToDo: Return Created() or BadRequest() in HttpResponse?
        let _result = dispatcher
            .toc
            .perform_points_op(&collection_name, PointsOperation::Upsert(operation))
            .await?;

        Ok(UpsertPointsResponse { num_points })
    })
    .await
}

#[derive(serde::Serialize)]
pub struct GetPointResponse {
    pub point: Point,
}

#[actix_web::get("/collections/{collection_name}/points/{id}")]
async fn get_point(
    path: web::Path<(String, String)>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let (collection_name, id) = path.into_inner();

        let point_id = match id.parse::<u64>() {
            Ok(id) => PointId::Id(id),
            Err(_) => PointId::Uuid(id.clone()),
        };

        let result = dispatcher
            .toc
            .retrieve_points(&collection_name, Some(vec![point_id]))
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

#[derive(serde::Serialize)]
pub struct ListPointsResponse {
    pub points: Vec<Point>,
}

#[actix_web::get("/collections/{collection_name}/points")]
async fn list_points(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();
        let result = dispatcher.toc.retrieve_points(&collection_name, None).await;
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
