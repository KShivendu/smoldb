#[cfg(test)]
mod tests;

pub mod apis;
pub mod args;
pub mod error;
pub mod types;
pub mod utils;

use crate::{
    apis::{create_collection, delete_collection, get_collection, retrieve_point, upsert_points},
    utils::log_latencies,
};
use args::parse_args;
use error::SmolBenchError;
use rand::Rng;

#[tokio::main]
async fn main() -> Result<(), SmolBenchError> {
    let args = parse_args();
    // println!("Parsed arguments: {:?}", &args);

    if !args.skip_create {
        let exists = get_collection(&args.uri, &args.collection_name)
            .await
            .is_ok();

        if exists {
            if args.skip_if_exists {
                println!(
                    "Collection '{}' already exists, skipping creation",
                    args.collection_name
                );
            } else {
                println!(
                    "Collection '{}' already exists, deleting it and creating a new one",
                    args.collection_name
                );
                delete_collection(&args.uri, &args.collection_name).await?;
                match create_collection(&args.uri, &args.collection_name).await {
                    Ok(_) => println!("Collection created successfully."),
                    Err(e) => return Err(SmolBenchError::CreateCollectionError(e.to_string()))?,
                }
            }
        } else {
            println!(
                "Collection '{}' does not exist, creating it",
                args.collection_name
            );
            match create_collection(&args.uri, &args.collection_name).await {
                Ok(_) => println!("Collection created successfully."),
                Err(e) => return Err(SmolBenchError::CreateCollectionError(e.to_string()))?,
            }
        }
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

        log_latencies(&batch_responses, "upsert").await?;
    }

    if !args.skip_query {
        let num_queries = args.num_points.min(1000) as u64;
        let mut rnd = rand::rng();
        let ids = (0..num_queries)
            .map(|_| rnd.random::<u64>() % args.num_points as u64) // Assume that IDs in the range [0, num_points) have been upserted
            .collect::<Vec<_>>();
        let responses = retrieve_point(&args.uri, &args.collection_name, ids).await?;
        println!(
            "Retrieved {} points from collection '{}':",
            responses.len(),
            args.collection_name,
        );

        log_latencies(&responses, "retrieve").await?;
    }

    Ok(())
}
