use std::sync::Arc;

use criterion::{BatchSize, Criterion};
use futures::{
    executor::block_on,
    stream::{self, StreamExt},
};

use crate::common::{
    benchmark_group, create_channel_service, create_collection, create_runtime, create_tempdir,
    generate_points,
};

pub fn write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "write");

    // Takes 619.19 ns on my machine
    // After hashring and tokio: 874.32 ns
    group.bench_function("single", |b| {
        let rt = create_runtime();
        let points = generate_points(1);
        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            block_on(async {
                create_collection("test_collection", &tempdir, channel_service, None).await
            })
        };
        b.to_async(&rt).iter_batched(
            setup,
            move |collection| {
                let points = points.clone();
                async move {
                    collection
                        .upsert_points(points.to_vec(), true)
                        .await
                        .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    // Takes 68.347 ms on my machine
    // After hashring and tokio: 172.99ms
    group.bench_function("concurrent", |b| {
        let rt = create_runtime();
        let num_points = 100_000;
        let num_threads = 16;
        let chunk_size = (num_points / num_threads) as usize;

        let setup = move || {
            let tempdir = create_tempdir();
            let channel_service = create_channel_service();
            let collection = block_on(async {
                create_collection("test_collection", &tempdir, channel_service, None).await
            });
            let collection_arc = Arc::new(collection);
            let points = generate_points(num_points);
            (collection_arc, points, chunk_size, num_threads)
        };
        b.to_async(&rt).iter_batched(
            setup,
            |(collection_arc, points, chunk_size, num_threads)| async move {
                stream::iter(points.chunks(chunk_size))
                    .map(|chunk| {
                        let collection_clone = collection_arc.clone();
                        let chunk = chunk.to_vec();
                        async move { collection_clone.upsert_points(chunk, true).await.unwrap() }
                    })
                    .buffer_unordered(num_threads as usize)
                    .collect::<Vec<_>>()
                    .await;
            },
            BatchSize::PerIteration,
        );
    });
}
