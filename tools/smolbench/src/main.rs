#[cfg(test)]
mod tests;

pub mod apis;
pub mod args;
pub mod error;
pub mod types;
pub mod utils;

use crate::{
    apis::{create_collection, delete_collection, exists_collection, read_point, upsert_points},
    utils::log_latencies,
};
use args::parse_args;
use error::SmolBenchError;
use futures::{
    stream::{self, StreamExt},
    TryStreamExt,
};
use rand::Rng;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), SmolBenchError> {
    let args = parse_args();
    // println!("Parsed arguments: {:?}", &args);

    if !args.skip_create {
        let exists = exists_collection(&args.uri, &args.collection_name).await?;

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
                delete_collection(&args.uri, &args.collection_name, true).await?;
                match create_collection(
                    &args.uri,
                    &args.collection_name,
                    args.skip_int_index,
                    args.skip_text_index,
                    true,
                )
                .await
                {
                    Ok(_) => println!("Collection created successfully."),
                    Err(e) => return Err(SmolBenchError::CreateCollectionError(e.to_string()))?,
                }
            }
        } else {
            println!(
                "Collection '{}' does not exist, creating it",
                args.collection_name
            );
            match create_collection(
                &args.uri,
                &args.collection_name,
                args.skip_int_index,
                args.skip_text_index,
                true,
            )
            .await
            {
                Ok(_) => println!("Collection created successfully."),
                Err(e) => return Err(SmolBenchError::CreateCollectionError(e.to_string()))?,
            }
        }
    }

    if !args.skip_upsert {
        println!(
            "Upserting {} points in batches of {} into collection '{}':",
            args.num_points, args.batch_size, args.collection_name
        );

        let batch_responses = upsert_points(
            &args.uri,
            &args.collection_name,
            args.num_points,
            args.batch_size,
            args.delay,
        )
        .await?;

        log_latencies(&batch_responses, args.p9, "server-side batched upsert").await?;
    }

    if !args.skip_read {
        let num_queries = args.num_points.min(1000) as u64;
        println!(
            "Reading {} points from collection '{}':",
            num_queries, args.collection_name,
        );

        let mut rnd = rand::rng();
        let ids = (0..num_queries)
            .map(|_| rnd.random::<u64>() % args.num_points as u64) // Assume that IDs in the range [0, num_points) have been upserted
            .collect::<Vec<_>>();
        let responses = read_point(&args.uri, &args.collection_name, ids).await?;

        log_latencies(&responses, args.p9, "server-side read").await?;
    }

    if !args.skip_query {
        let num_queries = (args.num_points as f32 * 0.01).max(100_f32) as u64;
        let limit = 100;
        println!(
            "Querying {} points with concurrency of {} from collection '{}' with price filter and limit of {}",
            num_queries, args.concurrent_queries, args.collection_name, limit,
        );

        let mut rnd = rand::rng();
        let futures = (0..num_queries)
            .map(|_| {
                let price_gte = rnd.random::<i64>() % args.num_points as i64;
                apis::query_points(
                    &args.uri,
                    &args.collection_name,
                    // ToDo: Benchmark with multiple filters at once
                    json!({
                        "filter": {
                            "key": "price",
                            "value": format!("{}", price_gte * 10),
                            "op": "gte",
                        },
                        "limit": limit,
                    }),
                )
            })
            .collect::<Vec<_>>();

        let responses = stream::iter(futures)
            .buffered(args.concurrent_queries)
            .try_collect::<Vec<_>>()
            .await?;

        log_latencies(&responses, args.p9, "server-side query").await?;
    }

    Ok(())
}
