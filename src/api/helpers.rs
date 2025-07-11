use actix_web::HttpResponse;
use serde::{Deserialize, Serialize};
use std::future::Future;
use tokio::time::Instant;

use crate::storage::error::CollectionResult;

type ResponseTime = f64;

#[derive(Serialize, Deserialize)]
pub struct ApiSuccessResponse<T> {
    pub result: T,
    pub time: ResponseTime,
}

#[derive(Serialize, Deserialize)]
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
            let res = ApiSuccessResponse {
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

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Success(ApiSuccessResponse<T>),
    Error(ApiErrorResponse),
}
