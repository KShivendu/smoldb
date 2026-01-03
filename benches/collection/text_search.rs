use std::{collections::BTreeMap, sync::Arc};

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_points, generate_text_queries, CONCURRENCY, NUM_POINTS_INDEXING,
    NUM_QUERIES, TEXT_FIELD,
};
use criterion::{Criterion, Throughput};
use futures::stream::{self, StreamExt};
use smoldb::storage::index::payload_index::IndexConfig;

// Perf when bench was first implemented: single=1.5804 µs, concurrent=5.7696 µs
pub fn text_query(c: &mut Criterion) {
    let mut group = benchmark_group(c, "query/text");
    let rt = create_runtime();

    // Single text query benchmark
    {
        let query = generate_text_queries(1).first().unwrap().clone();
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = rt.block_on(async {
            let payload_index = BTreeMap::from_iter([(TEXT_FIELD.to_string(), IndexConfig::Text)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
                true, // Wait for indexing to complete
            )
            .await
        });

        group.bench_function("single", |b| {
            b.to_async(&rt).iter(|| async {
                collection.query_points(query.clone(), true).await.unwrap();
            });
        });
    }

    // todo: Query a single batch of RW_BATCH_SIZE points while there are NUM_POINTS points in the collection
    // It needs support from the API to pass a batch of queries

    {
        let queries = generate_text_queries(NUM_QUERIES);
        let tempdir = create_tempdir();
        let channel_service = create_channel_service();
        let collection = rt.block_on(async {
            let payload_index = BTreeMap::from_iter([(TEXT_FIELD.to_string(), IndexConfig::Text)]);
            create_collection_with_points(
                &tempdir,
                channel_service,
                Some(payload_index),
                generate_points(NUM_POINTS),
                true, // Wait for indexing to complete
            )
            .await
        });

        // ToDo: Ensure that all points have been indexed before querying?

        let collection_arc = Arc::new(collection);

        // For querying, batch size is always 1 (for now)
        // So we can just run NUM_QUERIES queries in CONCURRENCY parallel with batch size=1
        group.throughput(Throughput::Elements(NUM_QUERIES as u64));
        group.bench_function("concurrent", |b| {
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
}
