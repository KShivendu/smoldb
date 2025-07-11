#[cfg(test)]
mod tests;

pub mod apis;
pub mod args;
pub mod error;
pub mod types;
pub mod utils;

use crate::{
    apis::{create_collection, retrieve_points, upsert_points},
    utils::log_latencies,
};
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

    if !args.skip_upsert {
        let batch_responses = upsert_points(
            &args.uri,
            &args.collection_name,
            args.num_points,
            args.batch_size,
            args.delay,
        )
        .await?;

        println!(
            "Upserted {} points in batches of {} into collection '{}':",
            args.num_points, args.batch_size, args.collection_name
        );

        log_latencies(&batch_responses).await?;
    }

    if !args.skip_query {
        let response = retrieve_points(&args.uri, &args.collection_name, None).await?;
        println!(
            "Retrieved {} points from collection '{}' in {}ms",
            response.result.points.len(),
            args.collection_name,
            response.time * 1000.0 // Convert seconds to milliseconds
        );
    }

    Ok(())
}
