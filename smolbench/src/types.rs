use serde::{Deserialize, Serialize};

type ResponseTime = f64;
pub type PointId = u64;

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

#[derive(Serialize, Deserialize)]
pub struct Point {
    pub id: usize,
    pub payload: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub struct Points {
    pub points: Vec<Point>,
}
