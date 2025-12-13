use criterion::Criterion;

use crate::common::{
    benchmark_group, create_runtime, create_segment_with_points, create_tempdir, generate_points,
};

pub fn single_read(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single read benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let segment = create_segment_with_points(&tempdir, None, generate_points(1));

    group.bench_function("single_read", |b| {
        b.to_async(&rt).iter(|| async {
            segment
                .get_points(Some(vec![smoldb::storage::segment::PointId::Id(0)]))
                .await
                .unwrap();
        })
    });
}

pub fn concurrent_read(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent read benchmarks");

    let rt = create_runtime();
    let tempdir = create_tempdir();

    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    let points = generate_points(num_points);
    let point_ids = points.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

    let segment = create_segment_with_points(&tempdir, None, points);

    group.bench_function("concurrent_read", |b| {
        b.to_async(&rt).iter(|| async {
            for chunk in point_ids.chunks(chunk_size) {
                segment.get_points(Some(chunk.to_vec())).await.unwrap();
            }
        })
    });
}
