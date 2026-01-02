use std::sync::Arc;

use criterion::{BatchSize, Criterion};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};

use crate::common::{
    benchmark_group, create_channel_service, create_collection_with_points, create_runtime,
    create_tempdir, generate_points,
};

// Perf in the beginning: ???
// Perf with hashring and tokio: 729.73 ns
pub fn single_read(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single read benchmarks");

    let rt = create_runtime();

    group.bench_function("single_read", |b| {
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            block_on(async {
                create_collection_with_points(&tempdir, channel_service, None, generate_points(1))
                    .await
            })
        };
        b.to_async(&rt).iter_batched(
            setup,
            |collection| async move {
                collection
                    .read_points(
                        Some(vec![smoldb::storage::segment::PointId::Id(0)]),
                        None,
                        true,
                    )
                    .await
                    .unwrap();
            },
            BatchSize::PerIteration,
        );
    });
}

// Perf in the beginning: ???
// Perf with hashring and tokio: 124.54ms (100_000 points, 4 threads, 2 shards; only 170x slower than single read)
pub fn concurrent_read(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent read benchmarks");

    let rt = create_runtime();
    let rt_handle = rt.handle().clone();
    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    group.bench_function("concurrent_read", |b| {
        let rt_handle = rt_handle.clone();
        let num_points = num_points;
        let chunk_size = chunk_size;
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let points = generate_points(num_points);
            let point_ids = points.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
            let collection = block_on(async {
                create_collection_with_points(&tempdir, channel_service, None, points).await
            });
            (Arc::new(collection), point_ids, chunk_size, num_threads)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection, point_ids, chunk_size, num_threads)| async move {
                stream::iter(point_ids.chunks(chunk_size))
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
                    .buffer_unordered(num_threads as usize)
                    .collect::<Vec<_>>()
                    .await;
            },
            BatchSize::PerIteration,
        );
    });
}
