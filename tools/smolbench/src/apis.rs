use crate::error::SmolBenchError;
use crate::types::{ApiResponse, ApiSuccessResponse, Point, PointId, Points};
use http::Uri;
use indicatif::ProgressStyle;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn wait_consensus_ready(url: &Uri) -> Result<(), SmolBenchError> {
    let now = std::time::Instant::now();

    while now.elapsed() < WAIT_TIMEOUT {
        let cluster_info = get_cluster_info(url).await?;

        match cluster_info {
            ApiResponse::Success(info) => {
                let role = info
                    .result
                    .get("raft_info")
                    .and_then(|r| r.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap();

                if !role.is_empty() {
                    // Don't confuse with raft crate's ready/lightready state
                    return Ok(());
                } else {
                    println!("Waiting for consensus to be ready...");
                }
            }
            ApiResponse::Error(err) => {
                return Err(SmolBenchError::ConsensusError(format!(
                    "Cluster not started: {}",
                    err.error
                )));
            }
        }

        sleep(Duration::from_millis(100)).await;
    }

    Err(SmolBenchError::ConsensusError(
        "Timed out waiting for consensus to start".to_string(),
    ))
}

async fn get_cluster_info(url: &Uri) -> Result<ApiResponse<Value>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client.get(format!("{url}/cluster")).send().await?;

    let body: ApiResponse<Value> = res.json().await?;

    Ok(body)
}

/// Strongly recommend to use `wait=true`
pub async fn create_collection(
    url: &Uri,
    collection_name: &str,
    skip_int_index: bool,
    wait: bool,
) -> Result<ApiSuccessResponse<bool>, SmolBenchError> {
    // First ensure that consensus is started
    crate::apis::wait_consensus_ready(&url).await?;

    let client = reqwest::Client::new();

    let payload_schema = if skip_int_index {
        json!({})
    } else {
        json!({
            "price": "int"
        })
    };

    let res = client
        .put(format!("{url}/collections/{collection_name}"))
        .json(&serde_json::json!({
            "params": "...",
            "payload_schema": payload_schema,
        }))
        .send()
        .await?;

    let body: ApiResponse<bool> = res.json().await?;

    let success_res = match body {
        ApiResponse::Success(body) => Ok(body),
        ApiResponse::Error(res) => Err(SmolBenchError::CreateCollectionError(res.error)),
    }?;

    println!("Waiting for collection '{}' to be created", collection_name);
    let now = std::time::Instant::now();

    if wait {
        while now.elapsed() < WAIT_TIMEOUT {
            let exists = exists_collection(url, collection_name).await?;

            if exists {
                return Ok(success_res);
            }

            if now.elapsed() >= WAIT_TIMEOUT {
                return Err(SmolBenchError::CreateCollectionError(
                    "Timed out waiting for collection creation".to_string(),
                ));
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    Ok(success_res)
}

async fn get_collection(
    url: &Uri,
    collection_name: &str,
) -> Result<ApiResponse<Value>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .get(format!("{url}/collections/{collection_name}"))
        .send()
        .await?;

    let body: ApiResponse<Value> = res.json().await?;

    Ok(body)
}

pub async fn exists_collection(url: &Uri, collection_name: &str) -> Result<bool, SmolBenchError> {
    // It's important to differentiate between collection not existing vs request/parsing
    match get_collection(url, collection_name).await {
        Ok(ApiResponse::Success(_collection)) => Ok(true),
        Ok(ApiResponse::Error(error_res)) => {
            if error_res.error
                == format!("Service error: Collection: {collection_name} doesn't exist")
            {
                Ok(false)
            } else {
                Err(SmolBenchError::CollectionExistsError(error_res.error))
            }
        }
        Err(e) => Err(e),
    }
}

/// Strongly recommend to use `wait=true`
pub async fn delete_collection(
    url: &Uri,
    collection_name: &str,
    wait: bool,
) -> Result<(), SmolBenchError> {
    // First ensure that consensus is started
    crate::apis::wait_consensus_ready(&url).await?;

    let client = reqwest::Client::new();

    let res = client
        .delete(format!("{url}/collections/{collection_name}"))
        .send()
        .await?;

    let body: ApiResponse<Value> = res.json().await?;
    let success_res = match body {
        ApiResponse::Success(_) => Ok(()),
        ApiResponse::Error(res) => Err(SmolBenchError::DeleteCollectionError(res.error)),
    }?;

    println!("Waiting for collection '{}' to be deleted", collection_name);
    let now = std::time::Instant::now();

    if wait {
        while now.elapsed() < WAIT_TIMEOUT {
            let deleted = !exists_collection(url, collection_name).await?;

            if deleted {
                return Ok(success_res);
            }

            if now.elapsed() >= WAIT_TIMEOUT {
                return Err(SmolBenchError::CreateCollectionError(
                    "Timed out waiting for collection deletion".to_string(),
                ));
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    Ok(success_res)
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
                    "price": i as i64 * 10,
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
                return Err(SmolBenchError::UpsertPointsError(res.error));
            }
        }

        pb.inc(points.len() as u64);

        if let Some(delay_ms) = delay {
            sleep(Duration::from_millis(delay_ms as u64)).await;
        }
    }

    Ok(results)
}

/// Read points by their IDs
pub async fn read_point(
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
            ApiResponse::Error(res) => Err(SmolBenchError::ReadPointsError(res.error))?,
        }
    }

    Ok(results)
}

/// Scroll through all points in a collection by pagination?
/// ToDo: Implement pagination logic in smoldb APIs
pub async fn read_points(
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
        ApiResponse::Error(res) => Err(SmolBenchError::ReadPointsError(res.error)),
    }
}
