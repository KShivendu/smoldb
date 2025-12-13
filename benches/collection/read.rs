use criterion::Criterion;

use crate::common::{
    create_channel_service, create_collection_with_points, create_runtime, create_tempdir,
    generate_points,
};

// Perf in the beginning: ???
// Perf with hashring and tokio: 729.73 ns
pub fn single_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("Single read benchmarks");
    group.sample_size(20);

    let rt = create_runtime();
    let tempdir = create_tempdir();
    let channel_service = create_channel_service();

    let collection = rt.block_on(async {
        create_collection_with_points(&tempdir, channel_service, None, generate_points(1)).await
    });

    group.bench_function("single_read", |b| {
        b.to_async(&rt).iter(|| async {
            collection
                .read_points(
                    Some(vec![smoldb::storage::segment::PointId::Id(0)]),
                    None,
                    true,
                )
                .await
                .unwrap();
        })
    });
}

// Perf in the beginning: ???
// Perf with hashring and tokio: 124.54ms (100_000 points, 4 threads, 2 shards; only 170x slower than single read)
pub fn concurrent_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent read benchmarks");
    group.sample_size(20);

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    let points = generate_points(num_points);
    let point_ids = points.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

    let channel_service = create_channel_service();

    let collection = rt.block_on(async {
        create_collection_with_points(&tempdir, channel_service, None, points).await
    });

    group.bench_function("concurrent_read", |b| {
        b.to_async(&rt).iter(|| async {
            for chunk in point_ids.chunks(chunk_size) {
                collection
                    .read_points(Some(chunk.to_vec()), None, true)
                    .await
                    .unwrap();
            }
        })
    });
}
