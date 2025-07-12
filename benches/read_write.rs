use criterion::{criterion_group, criterion_main, Criterion};
use serde_json::json;
use smoldb::storage::{
    collection::{Collection, CollectionConfig},
    segment::{Point, PointId},
};
use tempfile::TempDir;

// Takes 619.19 ns on my machine
// After hashring and tokio: 874.32 ns
pub fn single_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("Single write benchmarks");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = rt.block_on(async {
        Collection::init(
            "test_collection".to_string(),
            CollectionConfig {
                params: "...".to_string(),
            },
            tempdir.path(),
        )
        .await
        .unwrap()
    });

    let points = [Point {
        id: PointId::Id(0),
        payload: json!({ "msg": "Hello world" }),
    }];

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

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = rt.block_on(async {
        Collection::init(
            "test_collection".to_string(),
            CollectionConfig {
                params: "...".to_string(),
            },
            tempdir.path(),
        )
        .await
        .unwrap()
    });
    let collection_arc = std::sync::Arc::new(collection);

    let num_points = 100_000;
    let num_threads = 16;
    let chunk_size = (num_points / num_threads) as usize;

    let points: Vec<Point> = (0..num_points)
        .map(|i| Point {
            id: PointId::Id(i),
            payload: json!({ "msg": format!("Hello world {}", i) }),
        })
        .collect();

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

// Perf in the beginning: ???
// Perf with hashring and tokio: 729.73 ns
pub fn single_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("Single read benchmarks");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = rt.block_on(async {
        let collection = Collection::init(
            "test_collection".to_string(),
            CollectionConfig {
                params: "...".to_string(),
            },
            tempdir.path(),
        )
        .await
        .unwrap();

        let points = [Point {
            id: PointId::Id(0),
            payload: json!({ "msg": "Hello world" }),
        }];

        collection
            .upsert_points(points.to_vec(), true)
            .await
            .unwrap();

        collection
    });

    group.bench_function("single_read", |b| {
        b.to_async(&rt).iter(|| async {
            collection
                .get_points(Some(vec![PointId::Id(0)]), None, true)
                .await
                .unwrap();
        })
    });
}

// Perf in the beginning: ???
// Perf with hashring and tokio: 124.54ms (100_000 points, 4 threads, 2 shards; only 170x slower than single read)
fn concurrent_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent read benchmarks");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let num_points = 100_000;
    let num_threads = 4;
    let chunk_size = (num_points / num_threads) as usize;

    let points: Vec<Point> = (0..num_points)
        .map(|i| Point {
            id: PointId::Id(i),
            payload: json!({ "msg": format!("Hello world {}", i) }),
        })
        .collect();

    let point_ids = points.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

    let collection = rt.block_on(async {
        let collection = Collection::init(
            "test_collection".to_string(),
            CollectionConfig {
                params: "...".to_string(),
            },
            tempdir.path(),
        )
        .await
        .unwrap();

        collection.upsert_points(points, true).await.unwrap();

        collection
    });

    group.bench_function("concurrent_read", |b| {
        b.to_async(&rt).iter(|| async {
            for chunk in point_ids.chunks(chunk_size) {
                collection
                    .get_points(Some(chunk.to_vec()), None, true)
                    .await
                    .unwrap();
            }
        })
    });
}

criterion_group!(
    benches,
    single_write,
    concurrent_write,
    single_read,
    concurrent_read
);
criterion_main!(benches);
