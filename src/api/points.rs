use crate::api::collection::Dispatcher;
use actix_web::{
    Responder,
    web::{self, Json},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum PointId {
    Id(u64),
    Uuid(String),
}

impl PointId {
    pub fn into_string(&self) -> String {
        match self {
            PointId::Id(id) => id.to_string(),
            PointId::Uuid(uuid) => uuid.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Point {
    pub id: PointId,
    pub payload: serde_json::Value,
}

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
            "Failed to create collection '{}': {}",
            collection_name, e
        ));
    }

    actix_web::HttpResponse::Created().body(format!("{} points upserted successfully", num_points))
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
        .retrieve_points(&collection_name, &[point_id])
        .await;

    match result {
        Ok(points) if points.is_empty() => {
            return actix_web::HttpResponse::NotFound().body(format!(
                "Point with id '{}' not found in collection '{}'",
                id, collection_name
            ));
        }
        Ok(points) => {
            return actix_web::HttpResponse::Ok().json(&points[0]);
        }
        Err(e) => {
            return actix_web::HttpResponse::InternalServerError()
                .body(format!("Error retrieving point with id '{}': {}", id, e));
        }
    }
}
