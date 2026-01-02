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

// Perf when bench was first implemented: 31.807 µs
pub fn single_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single query benchmarks");

    let rt = create_runtime();

    let query = Query {
        filter: QueryFilter::new("price", Value::from(0), FilterOperator::Gte),
        limit: Some(10),
    };

    group.bench_function("single_query", |b| {
        let query = query.clone();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    Some(payload_index),
                    generate_points(1), // todo: Should have 100_000 points but query only for a single point
                )
                .await
            });
            (Arc::new(collection), query.clone())
        };
        b.to_async(&rt).iter_batched(
            setup,
            move |(collection_arc, query)| async move {
                collection_arc.query_points(query, true).await.unwrap();
            },
            BatchSize::PerIteration,
        );
    });
}

// Perf when bench was first implemented: 668.48 µs
pub fn concurrent_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent query benchmarks");

    let rt = create_runtime();
    let rt_handle = rt.handle().clone();
    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    group.bench_function("concurrent_query", |b| {
        let rt_handle = rt_handle.clone();
        let num_points = num_points;
        let num_threads = num_threads;
        let chunk_size = chunk_size;
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let points = generate_points(num_points);
            let collection = block_on(async {
                let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    Some(payload_index),
                    points,
                )
                .await
            });
            let collection_arc = Arc::new(collection);

            // Create queries for different chunks - filtering on different message values
            let queries: Vec<Query> = (0..num_threads)
                .map(|i| {
                    let start_id = i * chunk_size as u64;
                    Query {
                        filter: QueryFilter::new(
                            "price",
                            Value::from(start_id * 10),
                            FilterOperator::Gte,
                        ),
                        limit: Some(10),
                    }
                })
                .collect();

            (collection_arc, queries, num_threads)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection_arc, queries, num_threads)| async move {
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
