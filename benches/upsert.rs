use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use tempfile::TempDir;

use smoldb::storage::{
    content_manager::{Collection, CollectionConfig},
    segment::{Point, PointId},
};

// Takes 619.19 ns on my machine
pub fn single_upsert(c: &mut Criterion) {
    let mut group = c.benchmark_group("upserts");
    group.sample_size(20);

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
        b.iter(|| {
            // ToDo: Make async and benchmark that?
            collection.insert_points(&points).unwrap();
        })
    });
}

// Takes 68.347 ms on my machine
pub fn concurrent_upsert(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_upserts");
    group.sample_size(20);

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
        b.iter(|| {
            let mut handles = vec![];
            for chunk in points.chunks((num_points / num_threads) as usize) {
                let collection_clone = collection_arc.clone();
                let chunk_clone = chunk.to_vec();
                let handle = std::thread::spawn(move || {
                    collection_clone.insert_points(&chunk_clone).unwrap();
                });
                handles.push(handle);
            }
            for handle in handles {
                handle.join().expect("Thread panicked");
            }
        })
    });
}

criterion_group!(benches, single_upsert);
// criterion_group!(benches, concurrent_upsert);
criterion_main!(benches);
