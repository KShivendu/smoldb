use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use smoldb::storage::{
    content_manager::{Collection, CollectionConfig},
    segment::{Point, PointId},
};
use tempfile::TempDir;

// Takes 619.19 ns on my machine
// After hashring and tokio: 874.32 ns
pub fn single_upserts(c: &mut Criterion) {
    let mut group = c.benchmark_group("Single upsert benchmarks");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = Collection::init(
        "test_collection".to_string(),
        CollectionConfig {
            params: "...".to_string(),
        },
        tempdir.path(),
    )
    .unwrap();

    let points = [Point {
        id: PointId::Id(0),
        payload: json!({ "msg": "Hello world" }),
    }];

    group.bench_function("single_upsert", |b| {
        b.to_async(&rt).iter(|| async {
            collection.insert_points(&points).await.unwrap();
        })
    });
}

// Takes 68.347 ms on my machine
// After hashring and tokio: 172.99ms
pub fn concurrent_upsert(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent upsert benchmarks");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = Collection::init(
        "test_collection".to_string(),
        CollectionConfig {
            params: "...".to_string(),
        },
        tempdir.path(),
    )
    .unwrap();

    let num_points = 100_000;
    let num_threads = 4;

    let collection_arc = std::sync::Arc::new(collection);

    let points: Vec<Point> = (0..num_points)
        .map(|i| Point {
            id: PointId::Id(i),
            payload: json!({ "msg": format!("Hello world {}", i) }),
        })
        .collect();

    group.bench_function("concurrent_upsert", |b| {
        b.to_async(&rt).iter(|| async {
            for chunk in points.chunks((num_points / num_threads) as usize) {
                let collection_clone = collection_arc.clone();
                let chunk_clone = chunk.to_vec();
                collection_clone.insert_points(&chunk_clone).await.unwrap();
            }
        })
    });
}

// Perf in the beginning: ???
// Perf with hashring and tokio: 729.73 ns
pub fn single_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("read");
    group.sample_size(20);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let tempdir = TempDir::new().expect("Failed to create temporary directory");

    let collection = Collection::init(
        "test_collection".to_string(),
        CollectionConfig {
            params: "...".to_string(),
        },
        tempdir.path(),
    )
    .unwrap();

    let points = [Point {
        id: PointId::Id(0),
        payload: json!({ "msg": "Hello world" }),
    }];

    rt.block_on(async {
        collection.insert_points(&points).await.unwrap();
    });

    group.bench_function("single_read", |b| {
        b.to_async(&rt).iter(|| async {
            collection
                .get_points(Some(&[PointId::Id(0)]))
                .await
                .unwrap();
        })
    });
}

criterion_group!(benches, single_upserts, concurrent_upsert, single_read);
criterion_main!(benches);
