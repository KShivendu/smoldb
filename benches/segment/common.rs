use std::collections::BTreeMap;

use criterion::BenchmarkGroup;
use criterion::Criterion;
use serde_json::json;
use smoldb::storage::{
    index::payload_index::IndexConfig,
    segment::{Point, PointId, Segment},
};
use tempfile::TempDir;
use tokio::runtime::Runtime;

/// Creates a new multi-threaded tokio runtime for benchmarks
pub fn create_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Creates a temporary directory for benchmark storage
pub fn create_tempdir() -> TempDir {
    TempDir::new().expect("Failed to create temporary directory")
}

/// Creates a segment with default configuration
pub fn create_segment(
    tempdir: &TempDir,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
) -> Segment {
    let segments_dir = tempdir.path().join("segments");
    std::fs::create_dir_all(&segments_dir).expect("Failed to create segments directory");

    let payload_schema = payload_schema.unwrap_or_default();

    Segment::create(&segments_dir, payload_schema).expect("Failed to create segment")
}

/// Generates points with IDs from 0 to num_points-1 and payloads of the form "Hello world {id}"
pub fn generate_points(num_points: u64) -> Vec<Point> {
    (0..num_points)
        .map(|id| Point {
            id: PointId::Id(id),
            payload: json!({ "msg": format!("Hello world {}", id), "price": id as i64 * 10 }),
        })
        .collect()
}

/// Creates a segment and initializes it with the given points
pub fn create_segment_with_points(
    tempdir: &TempDir,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
    points: Vec<Point>,
) -> Segment {
    let segment = create_segment(tempdir, payload_schema);
    segment
        .insert_points(&points)
        .expect("Failed to insert points");
    segment
}

/// Creates a benchmark group with default configuration
pub fn benchmark_group<'a>(
    c: &'a mut Criterion,
    name: &str,
) -> BenchmarkGroup<'a, criterion::measurement::WallTime> {
    let mut group = c.benchmark_group(name);
    group.sample_size(20);
    group.significance_level(0.05);
    group.noise_threshold(0.05);
    group
}
