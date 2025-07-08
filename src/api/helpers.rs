use actix_web::HttpResponse;
use std::future::Future;
use tokio::time::Instant;

use crate::storage::error::CollectionResult;

type ResponseTime = f64;

#[derive(serde::Serialize)]
pub struct ApiResponse<T> {
    pub result: T,
    pub time: ResponseTime,
}

#[derive(serde::Serialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub time: ResponseTime,
}

/// Response wrapper for a `Future` returning `Result`.
pub async fn time<Fut, T>(future: Fut) -> HttpResponse
where
    Fut: Future<Output = CollectionResult<T>>,
    T: serde::Serialize,
{
    let instant = Instant::now();
    match future.await {
        Ok(r) => {
            let res = ApiResponse {
                result: r,
                time: instant.elapsed().as_secs_f64(),
            };

            actix_web::HttpResponse::Ok().json(res)
        }
        Err(e) => {
            let res = ApiErrorResponse {
                error: e.to_string(),
                time: instant.elapsed().as_secs_f64(),
            };

            actix_web::HttpResponse::InternalServerError().json(res)
        }
    }
}
