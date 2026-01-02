use std::sync::Arc;

use criterion::{BatchSize, Criterion, Throughput};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};

use crate::common::{
    benchmark_group, create_channel_service, create_collection, create_runtime, create_tempdir,
    generate_points, BATCH_SIZE, CONCURRENCY, NUM_POINTS,
};

pub fn write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "write");

    // Takes 619.19 ns on my machine
    // After hashring and tokio: 874.32 ns
    // Write a single point in an **empty** collection
    group.bench_function("single", |b| {
        let rt = create_runtime();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                create_collection("test_collection", &tempdir, channel_service, None).await
            });
            let single_point_batch = generate_points(1);
            (collection, single_point_batch)
        };
        b.to_async(&rt).iter_batched(
            setup,
            move |(collection, single_point_batch)| async move {
                collection
                    .upsert_points(single_point_batch.to_vec(), true)
                    .await
                    .unwrap();
            },
            BatchSize::PerIteration,
        );
    });

    group.throughput(Throughput::Elements(BATCH_SIZE as u64));

    // Write a single batch of BATCH_SIZE points in an **empty** collection
    group.bench_function("batch", |b| {
        let rt = create_runtime();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                create_collection("test_collection", &tempdir, channel_service, None).await
            });
            let write_batch = generate_points(BATCH_SIZE as u64).to_vec();
            (collection, write_batch)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection, write_batch)| async move {
                collection.upsert_points(write_batch, true).await.unwrap();
            },
            BatchSize::PerIteration,
        );
    });

    group.throughput(Throughput::Elements(NUM_POINTS));

    // Write NUM_POINTS points in CONCURRENCY parallel batches of BATCH_SIZE points in an **empty** collection
    // Takes 68.347 ms on my machine
    // After hashring and tokio: 172.99ms
    group.bench_function("concurrent", |b| {
        let rt = create_runtime();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                create_collection("test_collection", &tempdir, channel_service, None).await
            });
            let collection_arc = Arc::new(collection);
            let all_points = generate_points(NUM_POINTS);
            (collection_arc, all_points)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection_arc, all_points)| async move {
                stream::iter(all_points.chunks(BATCH_SIZE))
                    .map(|write_batch| {
                        let collection_clone = collection_arc.clone();
                        async move {
                            collection_clone
                                .upsert_points(write_batch.to_vec(), true)
                                .await
                                .unwrap()
                        }
                    })
                    .buffer_unordered(CONCURRENCY)
                    .collect::<Vec<_>>()
                    .await;
            },
            BatchSize::PerIteration,
        );
    });
}
