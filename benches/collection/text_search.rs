use std::{collections::BTreeMap, sync::Arc};

use criterion::Criterion;
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

// Perf when bench was first implemented: 1.5804 µs
pub fn single_text_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single text query benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();
    let channel_service = create_channel_service();
    let num_points = 100_000;

    let collection = rt.block_on(async {
        let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
        create_collection_with_points(
            &tempdir,
            channel_service,
            Some(payload_index),
            generate_points(num_points),
        )
        .await
    });
    let collection_arc = Arc::new(collection);

    let query = Query {
        filter: QueryFilter::new("text", Value::from("100"), FilterOperator::Eq),
        limit: Some(10),
    };

    group.bench_function("single_text_query", |b| {
        b.to_async(&rt).iter(|| async {
            collection_arc
                .query_points(query.clone(), true)
                .await
                .unwrap();
        })
    });
}

// Perf when bench was first implemented: 5.7696 µs
pub fn concurrent_text_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent text query benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    let points = generate_points(num_points);

    let channel_service = create_channel_service();

    let collection = rt.block_on(async {
        let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
        create_collection_with_points(&tempdir, channel_service, Some(payload_index), points).await
    });
    let collection_arc = Arc::new(collection);

    // Create queries for different chunks - filtering on different message values
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
        b.to_async(&rt).iter(|| async {
            for query in &queries {
                collection_arc
                    .query_points(query.clone(), true)
                    .await
                    .unwrap();
            }
        })
    });
}
