use serde::{Deserialize, Serialize};

type ResponseTime = f64;

#[derive(Deserialize)]
pub struct ApiSuccessResponse<T> {
    pub result: T,
    pub time: ResponseTime,
}

#[derive(Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub time: ResponseTime,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Success(ApiSuccessResponse<T>),
    Error(ApiErrorResponse),
}

#[derive(Serialize)]
pub struct Point {
    pub id: usize,
    pub payload: serde_json::Value,
}
