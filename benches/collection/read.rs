use std::sync::Arc;

use criterion::{BatchSize, Criterion, Throughput};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};
use smoldb::storage::segment::PointId;

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_points, BATCH_SIZE, CONCURRENCY, NUM_POINTS,
};

pub fn read(c: &mut Criterion) {
    let mut group = benchmark_group(c, "read");

    // Perf in the beginning: ???
    // Perf with hashring and tokio: 729.73 ns
    // Read a single point when there are NUM_POINTS points in the collection
    group.bench_function("single", |b| {
        let rt = create_runtime();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            block_on(async {
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    None,
                    generate_points(NUM_POINTS), // insert many points but query only for the last one
                )
                .await
            })
        };
        b.to_async(&rt).iter_batched(
            setup,
            |collection| async move {
                collection
                    .read_points(Some(vec![PointId::Id(NUM_POINTS - 1)]), None, true)
                    .await
                    .unwrap();
            },
            BatchSize::PerIteration,
        );
    });

    // Read 1 batch of BATCH_SIZE points while there are NUM_POINTS points in the collection
    group.throughput(Throughput::Elements(BATCH_SIZE as u64));
    group.bench_function("batch", |b| {
        let rt = create_runtime();
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                create_collection_with_points(
                    &tempdir,
                    channel_service,
                    None,
                    generate_points(NUM_POINTS),
                )
                .await
            });
            let read_batch = (0..BATCH_SIZE)
                .map(|i| PointId::Id(i as u64))
                .collect::<Vec<_>>();
            (collection, read_batch)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection, read_batch)| async move {
                collection
                    .read_points(Some(read_batch), None, true)
                    .await
                    .unwrap();
            },
            BatchSize::PerIteration,
        );
    });

    // Perf in the beginning: ???
    // Perf with hashring and tokio: 124.54ms (100_000 points, 4 threads, 2 shards; only 170x slower than single read)
    // Read NUM_POINTS points in NUM_THREADS parallel batches of BATCH_SIZE points
    group.throughput(Throughput::Elements(NUM_POINTS as u64));
    group.bench_function("concurrent", |b| {
        let rt = create_runtime();

        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let points = generate_points(NUM_POINTS);
            let all_point_ids = points.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
            let collection = block_on(async {
                create_collection_with_points(&tempdir, channel_service, None, points).await
            });
            (Arc::new(collection), all_point_ids)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection, all_point_ids)| async move {
                stream::iter(all_point_ids.chunks(BATCH_SIZE))
                    .map(|chunk| {
                        let collection_clone = collection.clone();
                        let chunk = chunk.to_vec();
                        async move {
                            collection_clone
                                .read_points(Some(chunk), None, true)
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
