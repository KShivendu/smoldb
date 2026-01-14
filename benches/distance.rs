use criterion::{criterion_group, criterion_main, Criterion};
use rand::{rngs::StdRng, Rng, SeedableRng};
use smoldb::storage::index::vector::simd::cosine_similarity_unit_simd;

fn distance_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("distance");

    let mut rng = StdRng::seed_from_u64(42);

    let dim = 1536;

    let random_vectors1: Vec<f64> = (0..dim).map(|_| rng.random::<f64>()).collect::<Vec<_>>();
    let random_vectors2: Vec<f64> = (0..dim).map(|_| rng.random::<f64>()).collect::<Vec<_>>();

    group.bench_function("cosine_similarity", |b| {
        b.iter(|| {
            cosine_similarity_unit_simd(&random_vectors1, &random_vectors2);
        });
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = distance_bench
);
criterion_main!(benches);
