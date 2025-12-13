use criterion::Criterion;

use crate::common::{benchmark_group, create_segment, create_tempdir, generate_points};

pub fn single_write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Single write benchmarks");

    let tempdir = create_tempdir();
    let segment = create_segment(&tempdir, None);

    let points = generate_points(1);

    group.bench_function("single_write", |b| {
        b.iter(|| {
            segment
                .insert_points(&points)
                .expect("Failed to insert points");
        })
    });
}

pub fn concurrent_write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "Concurrent write benchmarks");

    let tempdir = create_tempdir();
    let segment = create_segment(&tempdir, None);

    let num_points = 100_000;
    let num_chunks = 16;
    let chunk_size = (num_points / num_chunks) as usize;

    let points = generate_points(num_points);

    group.bench_function("concurrent_write", |b| {
        b.iter(|| {
            for chunk in points.chunks(chunk_size) {
                segment
                    .insert_points(chunk)
                    .expect("Failed to insert points");
            }
        })
    });
}
