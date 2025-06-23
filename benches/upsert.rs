use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use tempfile::TempDir;

use smoldb::storage::{
    content_manager::{Collection, CollectionConfig},
    segment::{Point, PointId},
};

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

criterion_group!(benches, single_upsert);
criterion_main!(benches);
