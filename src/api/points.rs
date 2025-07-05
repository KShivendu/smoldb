use crate::{
    api::collection::Dispatcher,
    storage::segment::{Point, PointId},
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

#[actix_web::put("/collections/{collection_name}/points")]
async fn upsert_points(
    collection_name: web::Path<String>,
    operation: Json<UpsertPoints>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    let collection_name = collection_name.into_inner();
    let operation = operation.into_inner();

    let num_points = operation.points.len();

    // ToDo: Push this to consensus instead of directly committing locally?
    let result = dispatcher
        .toc
        .perform_points_op(&collection_name, PointsOperation::Upsert(operation))
        .await;

    if let Err(e) = result {
        return actix_web::HttpResponse::BadRequest().body(format!(
            "Failed to create collection '{collection_name}': {e}"
        ));
    }

    actix_web::HttpResponse::Created().body(format!("{num_points} points upserted successfully"))
}

#[actix_web::get("/collections/{collection_name}/points/{id}")]
async fn get_point(
    path: web::Path<(String, String)>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
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
        Ok(points) if points.is_empty() => actix_web::HttpResponse::NotFound().body(format!(
            "Point with id '{id}' not found in collection '{collection_name}'"
        )),
        Ok(points) => actix_web::HttpResponse::Ok().json(&points[0]),
        Err(e) => actix_web::HttpResponse::InternalServerError()
            .body(format!("Error retrieving point with id '{id}': {e}")),
    }
}

#[actix_web::get("/collections/{collection_name}/points")]
async fn list_points(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    let collection_name = collection_name.into_inner();
    let result = dispatcher.toc.retrieve_points(&collection_name, None).await;
    match result {
        Ok(points) => {
            if points.is_empty() {
                actix_web::HttpResponse::NotFound()
                    .body(format!("No points found in collection '{collection_name}'"))
            } else {
                actix_web::HttpResponse::Ok().json(points)
            }
        }
        Err(e) => actix_web::HttpResponse::InternalServerError().body(format!(
            "Error listing points in collection '{collection_name}': {e}"
        )),
    }
}
