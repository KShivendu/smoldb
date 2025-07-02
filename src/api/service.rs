use actix_web::{HttpResponse, Responder};
use serde::Serialize;

#[derive(Serialize)]
struct RootApiResponse {
    title: String,
    version: String,
}

#[actix_web::get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().json(RootApiResponse {
        title: "Smol DB".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
