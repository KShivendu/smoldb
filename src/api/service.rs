use actix_web::{get, HttpResponse, Responder};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct RootApiResponse {
    #[schema(example = "Smoldb")]
    title: String,
    #[schema(example = "0.1.0")]
    version: String,
}

#[utoipa::path(
    responses(
        (status = 200, description = "Get info about the smoldb instance", body = RootApiResponse),
    ),
)]
#[get("/")]
async fn root_api() -> impl Responder {
    HttpResponse::Ok().json(RootApiResponse {
        title: "Smol DB".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
