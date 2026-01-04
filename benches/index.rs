use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkGroup, Criterion};
use serde_json::Value;
use smoldb::storage::index::{
    integer::IntegerIndex, payload_index::FieldIndexTrait, text::TextIndex,
};
use tempfile::TempDir;

const BATCH_SIZE: usize = 1000;

/// Creates a benchmark group with default configuration
fn benchmark_group<'a>(
    c: &'a mut Criterion,
    name: &str,
) -> BenchmarkGroup<'a, criterion::measurement::WallTime> {
    let mut group = c.benchmark_group(name);
    group.sample_size(100);
    group.measurement_time(Duration::from_secs(5));
    group.warm_up_time(Duration::from_secs(3));
    group.significance_level(0.05);
    group.noise_threshold(0.05);
    group
}

/// Creates a temporary database for benchmark storage
fn create_temp_db() -> (sled::Db, TempDir) {
    let tempdir = TempDir::new().expect("Failed to create temporary directory");
    let db = sled::open(tempdir.path()).unwrap();
    (db, tempdir)
}

/// Generates integer values for indexing benchmarks
fn generate_integer_values(num_points: usize) -> Vec<Value> {
    (0..num_points)
        .map(|i| Value::Number((i as i64 * 10).into()))
        .collect()
}

/// Generates text values for indexing benchmarks
fn generate_text_values(num_points: usize) -> Vec<Value> {
    (0..num_points)
        .map(|i| Value::String(format!("foo bar {}", i)))
        .collect()
}

// Integer index benchmarks

pub fn int_indexing(c: &mut Criterion) {
    let mut group = benchmark_group(c, "index/int");

    let num_points = BATCH_SIZE as u64;
    let point_ids: Vec<u64> = (0..num_points).collect();
    let values = generate_integer_values(num_points as usize);

    // For now only upserting BATCH_SIZE points at a time, but must upsert NUM_POINTS points in future
    // once indexing is faster due to batching.
    {
        let (db, _tempdir) = create_temp_db();
        let index = IntegerIndex::open(&db, "price", false).unwrap();

        group.bench_function("batch", |b| {
            b.iter(|| {
                index.add_points(&point_ids, &values).unwrap();
            });
        });
    }

    {
        let (db, _tempdir) = create_temp_db();
        let index_with_in_mem = IntegerIndex::open(&db, "price", true).unwrap();

        group.bench_function("in_memory/batch", |b| {
            b.iter(|| {
                index_with_in_mem.add_points(&point_ids, &values).unwrap();
            });
        });
    }
}

// Text index benchmarks

pub fn text_indexing(c: &mut Criterion) {
    let mut group = benchmark_group(c, "index/text");

    let num_points = BATCH_SIZE as u64;
    let point_ids: Vec<u64> = (0..num_points).collect();
    let values = generate_text_values(num_points as usize);

    // For now only upserting BATCH_SIZE points at a time, but must upsert NUM_POINTS points in future
    // once indexing is faster due to batching.
    group.bench_function("batch", |b| {
        let (db, _tempdir) = create_temp_db();
        let index = TextIndex::open(&db, "description", false).unwrap();

        b.iter(|| {
            index.add_points(&point_ids, &values).unwrap();
        });
    });

    group.bench_function("in_memory/batch", |b| {
        let (db, _tempdir) = create_temp_db();
        let index_with_in_mem = TextIndex::open(&db, "description", true).unwrap();

        b.iter(|| {
            index_with_in_mem.add_points(&point_ids, &values).unwrap();
        });
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = int_indexing, text_indexing
);
criterion_main!(benches);
