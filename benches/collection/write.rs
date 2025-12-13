use std::sync::Arc;

use criterion::Criterion;

use crate::common::{
    create_channel_service, create_collection, create_runtime, create_tempdir, generate_points,
};

// Takes 619.19 ns on my machine
// After hashring and tokio: 874.32 ns
pub fn single_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("Single write benchmarks");
    group.sample_size(20);

    let rt = create_runtime();
    let tempdir = create_tempdir();
    let channel_service = create_channel_service();

    let collection = rt.block_on(async {
        create_collection("test_collection", &tempdir, channel_service.clone(), None).await
    });

    let points = generate_points(1);

    group.bench_function("single_write", |b| {
        b.to_async(&rt).iter(|| async {
            collection
                .upsert_points(points.to_vec(), true)
                .await
                .unwrap();
        })
    });
}

// Takes 68.347 ms on my machine
// After hashring and tokio: 172.99ms
pub fn concurrent_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent write benchmarks");
    group.sample_size(20);

    let rt = create_runtime();
    let tempdir = create_tempdir();
    let channel_service = create_channel_service();

    let collection = rt.block_on(async {
        create_collection("test_collection", &tempdir, channel_service, None).await
    });
    let collection_arc = Arc::new(collection);

    let num_points = 100_000;
    let num_threads = 16;
    let chunk_size = (num_points / num_threads) as usize;

    let points = generate_points(num_points);

    group.bench_function("concurrent_write", |b| {
        b.to_async(&rt).iter(|| async {
            for chunk in points.chunks(chunk_size) {
                let collection_clone = collection_arc.clone();
                collection_clone
                    .upsert_points(chunk.to_vec(), true)
                    .await
                    .unwrap();
            }
        })
    });
}
