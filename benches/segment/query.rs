use std::collections::BTreeMap;

use criterion::Criterion;

use crate::common::{
    benchmark_group, create_runtime, create_segment_with_points, create_tempdir, generate_points,
};
use smoldb::{
    api::points::Query,
    storage::index::{
        filter::{FilterOperator, QueryFilter},
        payload_index::IndexConfig,
    },
};

pub fn single_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single query benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
    let segment = create_segment_with_points(&tempdir, Some(payload_index), generate_points(1));

    let query = Query {
        filter: QueryFilter::new("price", "0", FilterOperator::Gte),
        limit: Some(10),
    };

    group.bench_function("single_query", |b| {
        b.to_async(&rt).iter(|| async {
            segment.query_points(query.clone()).await.unwrap();
        })
    });
}

pub fn concurrent_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent query benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    let points = generate_points(num_points);

    let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
    let segment = create_segment_with_points(&tempdir, Some(payload_index), points);

    // Create queries for different chunks - filtering on different price values
    let queries: Vec<Query> = (0..num_threads)
        .map(|i| {
            let start_id = i * chunk_size as u64;
            Query {
                filter: QueryFilter::new(
                    "price",
                    format!("{}", start_id * 10),
                    FilterOperator::Gte,
                ),
                limit: Some(10),
            }
        })
        .collect();

    group.bench_function("concurrent_query", |b| {
        b.to_async(&rt).iter(|| async {
            for query in &queries {
                segment.query_points(query.clone()).await.unwrap();
            }
        })
    });
}
