pub mod args;
pub mod error;
pub mod types;

use crate::types::{ApiResponse, ApiSuccessResponse, Point};
use args::parse_args;
use error::SmolBenchError;
use http::Uri;
use serde_json::{json, Value};

async fn create_collection(
    url: &Uri,
    collection_name: &str,
) -> Result<ApiSuccessResponse<Value>, SmolBenchError> {
    let client = reqwest::Client::new();

    let res = client
        .put(format!("{url}/collections/{collection_name}"))
        .json(&serde_json::json!({
            "params": "..."
        }))
        .send()
        .await?;

    let body: ApiResponse<Value> = res.json().await?;

    match body {
        ApiResponse::Success(body) => {
            dbg!(&body.result);
            Ok(body)
        }
        ApiResponse::Error(res) => Err(SmolBenchError::CreateCollectionError(res.error)),
    }
}

async fn upsert_points(
    url: &Uri,
    collection_name: &str,
    num_points: usize,
    batch_size: usize,
) -> Result<Vec<ApiSuccessResponse<Value>>, SmolBenchError> {
    let client = reqwest::Client::new();
    let num_batches = num_points.div_ceil(batch_size);

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
    }

    Ok(results)
}

#[tokio::main]
async fn main() -> Result<(), SmolBenchError> {
    let args = parse_args();
    // println!("Parsed arguments: {:?}", &args);

    // ToDo: Avoid calling this once collection exists API is introduced?
    match create_collection(&args.uri, &args.collection_name).await {
        Ok(_) => println!("Collection created successfully."),
        Err(e) => eprintln!("Ignoring error while creating collection: {e}"),
    }

    let _batch_responses = upsert_points(
        &args.uri,
        &args.collection_name,
        args.num_points,
        args.batch_size,
    )
    .await?;

    println!(
        "Upserted {} points in batches of {} into collection '{}'",
        args.num_points, args.batch_size, args.collection_name
    );

    Ok(())
}
