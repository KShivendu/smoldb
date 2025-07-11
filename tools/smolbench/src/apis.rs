use crate::error::SmolBenchError;
use crate::types::{ApiResponse, ApiSuccessResponse, Point, PointId, Points};
use http::Uri;
use indicatif::ProgressStyle;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;

pub async fn create_collection(
    url: &Uri,
    collection_name: &str,
) -> Result<ApiSuccessResponse<bool>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .put(format!("{url}/collections/{collection_name}"))
        .json(&serde_json::json!({
            "params": "..."
        }))
        .send()
        .await?;

    let body: ApiResponse<bool> = res.json().await?;

    match body {
        ApiResponse::Success(body) => Ok(body),
        ApiResponse::Error(res) => Err(SmolBenchError::CreateCollectionError(res.error)),
    }
}

pub async fn get_collection(
    url: &Uri,
    collection_name: &str,
) -> Result<ApiSuccessResponse<Value>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .get(format!("{url}/collections/{collection_name}"))
        .send()
        .await?;

    let body: ApiResponse<Value> = res.json().await?;

    match body {
        ApiResponse::Success(body) => Ok(body),
        ApiResponse::Error(res) => Err(SmolBenchError::RetrievePointsError(res.error)),
    }
}

pub async fn delete_collection(url: &Uri, collection_name: &str) -> Result<(), SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .delete(format!("{url}/collections/{collection_name}"))
        .send()
        .await?;

    if res.status().is_success() {
        Ok(())
    } else {
        let body: ApiResponse<Value> = res.json().await?;
        match body {
            ApiResponse::Success(_) => Ok(()),
            ApiResponse::Error(res) => Err(SmolBenchError::DeleteCollectionError(res.error)),
        }
    }
}

pub async fn upsert_points(
    url: &Uri,
    collection_name: &str,
    num_points: usize,
    batch_size: usize,
    delay: Option<usize>,
) -> Result<Vec<ApiSuccessResponse<Value>>, SmolBenchError> {
    let client = reqwest::Client::new();
    let num_batches = num_points.div_ceil(batch_size);

    let pb = indicatif::ProgressBar::new(num_points as u64);
    let progress_style = ProgressStyle::default_bar()
        .template("{msg} [{elapsed_precise}] {wide_bar} [{per_sec:>3}] {pos}/{len} (eta:{eta})")
        .expect("Failed to create progress style");
    pb.set_style(progress_style);

    let mut results = Vec::with_capacity(num_batches);

    for batch in 0..num_batches {
        let start = batch * batch_size;
        let end = std::cmp::min(start + batch_size, num_points);

        let batch_ts = chrono::Utc::now();

        let points: Vec<Point> = (start..end)
            .map(|i| Point {
                id: i,
                payload: json!({
                    "text": format!("Point {}", i),
                    "timestamp": batch_ts.to_rfc3339(),
                }),
            })
            .collect();

        let res = client
            .put(format!("{url}/collections/{collection_name}/points"))
            .json(&json!({
                "points": points,
            }))
            .send()
            .await?;

        let text = res.text().await?;

        let body: ApiResponse<Value> = serde_json::from_str(&text)?;

        match body {
            ApiResponse::Success(body) => results.push(body),
            ApiResponse::Error(res) => {
                return Err(SmolBenchError::CreateCollectionError(res.error));
            }
        }

        pb.inc(points.len() as u64);

        if let Some(delay_ms) = delay {
            sleep(Duration::from_millis(delay_ms as u64)).await;
        }
    }

    Ok(results)
}

/// Retrieve points by their IDs
pub async fn retrieve_point(
    url: &Uri,
    collection_name: &str,
    ids: Vec<u64>,
) -> Result<Vec<ApiSuccessResponse<Value>>, SmolBenchError> {
    let client = reqwest::Client::new();
    let mut results = Vec::with_capacity(ids.len());

    for id in ids {
        let res = client
            .get(format!("{url}/collections/{collection_name}/points/{id}"))
            .send()
            .await?;

        let body: ApiResponse<Value> = res.json().await?;

        match body {
            ApiResponse::Success(body) => {
                results.push(body);
            }
            ApiResponse::Error(res) => Err(SmolBenchError::RetrievePointsError(res.error))?,
        }
    }

    Ok(results)
}

/// Scroll through all points in a collection by pagination?
/// ToDo: Implement pagination logic in smoldb APIs
pub async fn retrieve_points(
    url: &Uri,
    collection_name: &str,
    _ids: Option<Vec<PointId>>, // ToDo: Use this parameter to filter points
) -> Result<ApiSuccessResponse<Points>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .get(format!("{url}/collections/{collection_name}/points"))
        .send()
        .await?;

    let body: ApiResponse<Points> = res.json().await?;

    match body {
        ApiResponse::Success(body) => Ok(body),
        ApiResponse::Error(res) => Err(SmolBenchError::RetrievePointsError(res.error)),
    }
}
