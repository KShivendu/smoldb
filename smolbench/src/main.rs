pub mod apis;
pub mod args;
pub mod error;
pub mod types;

use crate::apis::{create_collection, upsert_points};
use args::parse_args;
use error::SmolBenchError;

#[tokio::main]
async fn main() -> Result<(), SmolBenchError> {
    let args = parse_args();
    // println!("Parsed arguments: {:?}", &args);

    // ToDo: Avoid calling this once collection exists API is introduced?
    match create_collection(&args.uri, &args.collection_name).await {
        Ok(_) => println!("Collection created successfully."),
        Err(e) => eprintln!("Ignoring error while creating collection: {e}"),
    }

    let batch_responses = upsert_points(
        &args.uri,
        &args.collection_name,
        args.num_points,
        args.batch_size,
    )
    .await?;

    let latencies = batch_responses
        .iter()
        .map(|res| res.time * 1000.0) // Convert s to ms
        .map(|latency| latency as f64)
        .collect::<Vec<_>>();

    let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let min_latency = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_latency = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!("Avg batch server latency: {:.2} ms", avg_latency);
    println!("Min batch server latency: {:.2} ms", min_latency);
    println!("Max batch server latency: {:.2} ms", max_latency);

    println!(
        "Upserted {} points in batches of {} into collection '{}'",
        args.num_points, args.batch_size, args.collection_name
    );

    Ok(())
}
