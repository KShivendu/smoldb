use std::{collections::BTreeMap, sync::Arc};

use criterion::{BatchSize, Criterion};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};
use serde_json::Value;

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_points,
};
use smoldb::{
    api::points::Query,
    storage::index::{
        filter::{FilterOperator, QueryFilter},
        payload_index::IndexConfig,
    },
};

// Perf when bench was first implemented: single=1.5804 µs, concurrent=5.7696 µs
pub fn text_query_benchmarks(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Text query benchmarks");

    let rt = create_runtime();
    let rt_handle = rt.handle().clone();
    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    // Single text query benchmark
    let single_query = Query {
        filter: QueryFilter::new("text", Value::from("100"), FilterOperator::Eq),
        limit: Some(10),
    };

    group.bench_function("single_text_query", |b| {
        let num_points = num_points;
        let query = single_query.clone();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let points = generate_points(num_points);
            let collection = block_on(async {
                let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    Some(payload_index),
                    points,
                )
                .await
            });
            Arc::new(collection)
        };
        b.to_async(&rt).iter_batched(
            setup,
            move |collection_arc| {
                let query = query.clone();
                async move {
                    collection_arc.query_points(query, true).await.unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    // Concurrent text query benchmark
    let queries: Vec<Query> = (0..num_threads)
        .map(|i| {
            let start_id = i * chunk_size as u64;
            Query {
                filter: QueryFilter::new(
                    "text",
                    Value::from(format!("{start_id}")),
                    FilterOperator::Gte,
                ),
                limit: Some(10),
            }
        })
        .collect();

    group.bench_function("concurrent_text_query", |b| {
        let rt_handle = rt_handle.clone();
        let num_points = num_points;
        let queries = queries.clone();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let points = generate_points(num_points);
            let collection = block_on(async {
                let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    Some(payload_index),
                    points,
                )
                .await
            });
            (Arc::new(collection), queries.clone(), num_threads)
        };
        b.to_async(&rt).iter_batched(
            setup,
            move |(collection_arc, queries, num_threads)| async move {
                stream::iter(queries)
                    .map(|query| {
                        let collection_clone = collection_arc.clone();
                        async move { collection_clone.query_points(query, true).await.unwrap() }
                    })
                    .buffer_unordered(num_threads as usize)
                    .collect::<Vec<_>>()
                    .await;
            },
            BatchSize::PerIteration,
        );
    });
}
