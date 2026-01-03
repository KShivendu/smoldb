use std::{collections::BTreeMap, sync::Arc};

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_int_queries, generate_points, CONCURRENCY, NUM_POINTS, NUM_QUERIES,
};
use criterion::Criterion;
use futures::stream::{self, StreamExt};
use smoldb::storage::index::payload_index::IndexConfig;

pub fn int_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "query/int");

    let rt = create_runtime();

    // Perf when bench was first implemented: 31.807 µs
    // Query a single point while there are NUM_POINTS points in the collection
    {
        let query = generate_int_queries(1).first().unwrap().clone();
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = rt.block_on(async {
            let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
            )
            .await
        });
        group.bench_function("single", |b| {
            b.to_async(&rt).iter(|| async {
                collection.query_points(query.clone(), true).await.unwrap();
            });
        });
    }

    // ToDo: Query a single batch of RW_BATCH_SIZE points while there are NUM_POINTS points in the collection
    // It needs support from the API to pass a batch of queries

    // Perf when bench was first implemented: 668.48 µs
    // For querying, batch size is always 1 (for now)
    // So we can just run num_queries queries in CONCURRENCY parallel with batch size=1
    {
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = rt.block_on(async {
            let payload_index = BTreeMap::from_iter([("price".to_string(), IndexConfig::Int)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
            )
            .await
        });

        let collection_arc = Arc::new(collection);

        // Create queries for different chunks
        let queries = generate_int_queries(NUM_QUERIES);

        group.bench_function("concurrent", |b: &mut criterion::Bencher<'_>| {
            b.to_async(&rt).iter(|| async {
                let queries = queries.clone();
                stream::iter(queries)
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
}
