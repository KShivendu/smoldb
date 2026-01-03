use std::{collections::BTreeMap, sync::Arc};

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_points, generate_text_queries, CONCURRENCY, NUM_POINTS, NUM_QUERIES,
};
use criterion::{Criterion, Throughput};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};
use smoldb::storage::index::payload_index::IndexConfig;

// Perf when bench was first implemented: single=1.5804 µs, concurrent=5.7696 µs
pub fn text_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "query/text");

    // Single text query benchmark
    group.bench_function("single", |b| {
        let rt = create_runtime();
        let query = generate_text_queries(1).first().unwrap().clone();
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = block_on(async {
            let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
            )
            .await
        });
        b.to_async(&rt).iter(|| async {
            collection.query_points(query.clone(), true).await.unwrap();
        });
    });

    // todo: Query a single batch of RW_BATCH_SIZE points while there are NUM_POINTS points in the collection
    // It needs support from the API to pass a batch of queries

    group.throughput(Throughput::Elements(NUM_QUERIES as u64));

    // For querying, batch size is always 1 (for now)
    // So we can just run NUM_QUERIES queries in CONCURRENCY parallel with batch size=1
    group.bench_function("concurrent", |b| {
        let rt = create_runtime();
        let queries = generate_text_queries(NUM_QUERIES);
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = block_on(async {
            let payload_index = BTreeMap::from_iter([("text".to_string(), IndexConfig::Text)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
            )
            .await
        });
        let collection_arc = Arc::new(collection);
        b.to_async(&rt).iter(|| async {
            stream::iter(queries.clone())
                .map(|query| {
                    let collection_clone = collection_arc.clone();
                    async move { collection_clone.query_points(query, true).await.unwrap() }
                })
                .buffer_unordered(CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
        });
    });
}
