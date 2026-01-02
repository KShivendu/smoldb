use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};

use criterion::BenchmarkGroup;
use criterion::Criterion;
use serde_json::json;
use serde_json::Value;
use smoldb::api::points::Query;
use smoldb::storage::index::filter::FilterOperator;
use smoldb::storage::index::filter::QueryFilter;
use smoldb::{
    channel_service::ChannelService,
    storage::{
        collection::{Collection, CollectionConfig},
        index::payload_index::IndexConfig,
        segment::{Point, PointId},
    },
};
use tempfile::TempDir;
use tokio::runtime::Runtime;

pub const NUM_POINTS: u64 = 100_000;
// Batch size for reading and writing (but not querying)
pub const BATCH_SIZE: usize = 1000;
// Number of queries to execute in total
pub const NUM_QUERIES: usize = 1000;
pub const CONCURRENCY: usize = 16;

pub const TEXT_FIELD: &str = "description";
pub const INT_FIELD: &str = "price";

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

/// Creates a default channel service
pub fn create_channel_service() -> Arc<ChannelService> {
    Arc::new(ChannelService::default())
}

/// Creates a collection with default configuration
pub async fn create_collection(
    collection_name: &str,
    tempdir: &TempDir,
    channel_service: Arc<ChannelService>,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
) -> Collection {
    let config = CollectionConfig {
        params: "...".to_string(),
        payload_schema,
    };

    Collection::init(
        collection_name.to_string(),
        config,
        tempdir.path(),
        channel_service,
    )
    .await
    .unwrap()
}

/// Generates points with IDs from 0 to num_points-1 and payloads of the form "Hello world {id}"
pub fn generate_points(num_points: u64) -> Vec<Point> {
    (0..num_points)
        .map(|id| Point {
            id: PointId::Id(id),
            payload: json!({ TEXT_FIELD: format!("Hello world {}", id), INT_FIELD: id as i64 * 10 }),
        })
        .collect()
}

/// todo: not used in the benchmarks yet
/// Generates a random point with a random ID and payload
#[allow(dead_code)]
pub fn generate_random_points(num_points: usize) -> Vec<Point> {
    (0..num_points)
        .map(|_| {
            let id = rand::random_range(0..NUM_POINTS);
            let payload =
                json!({ TEXT_FIELD: format!("Hello world {}", id), INT_FIELD: id as i64 * 10 });
            Point {
                id: PointId::Id(id),
                payload,
            }
        })
        .collect::<Vec<_>>()
}

/// Generate queries for integer index
pub fn generate_int_queries(num_queries: usize) -> Vec<Query> {
    (0..num_queries)
        .map(|i| Query {
            filter: QueryFilter::new(INT_FIELD, Value::from(i * 10), FilterOperator::Gte),
            limit: Some(10),
        })
        .collect::<Vec<_>>()
}

/// Generate queries for text index
pub fn generate_text_queries(num_queries: usize) -> Vec<Query> {
    (0..num_queries)
        .map(|i| Query {
            filter: QueryFilter::new(
                TEXT_FIELD,
                Value::from(format!("Hello world {}", i)),
                FilterOperator::Eq,
            ),
            limit: Some(10),
        })
        .collect::<Vec<_>>()
}

/// Creates a collection and initializes it with the given points
pub async fn create_collection_with_points(
    tempdir: &TempDir,
    channel_service: Arc<ChannelService>,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
    points: Vec<Point>,
    wait_for_indexing: bool,
) -> Collection {
    let collection =
        create_collection("test_collection", tempdir, channel_service, payload_schema).await;
    collection.upsert_points(points, true).await.unwrap();

    if wait_for_indexing {
        let start_time = std::time::Instant::now();
        loop {
            let pending_indexing_count = collection.get_pending_indexing_count(None).await;
            if pending_indexing_count > 0 {
                eprintln!("Waiting for indexing to complete... {pending_indexing_count} points pending, elapsed: {:.2?}", start_time.elapsed());
                tokio::time::sleep(Duration::from_millis(100)).await;
            } else {
                eprintln!("Indexing completed! Elapsed: {:.2?}", start_time.elapsed());
                break;
            }
        }
    }

    collection
}

/// Creates a benchmark group with default configuration
pub fn benchmark_group<'a>(
    c: &'a mut Criterion,
    name: &str,
) -> BenchmarkGroup<'a, criterion::measurement::WallTime> {
    let mut group = c.benchmark_group(name);
    group.sample_size(100); // default is 100
    group.measurement_time(Duration::from_secs(5)); // default is 5s
    group.warm_up_time(Duration::from_secs(3)); // default is 3s
    group.significance_level(0.05);
    group.noise_threshold(0.05);
    group
}

/// Creates a temporary database for benchmark storage
pub fn create_temp_db() -> (sled::Db, TempDir) {
    let tempdir = create_tempdir();
    let db = sled::open(tempdir.path()).unwrap();
    (db, tempdir)
}

/// Generates integer values for indexing benchmarks
pub fn generate_integer_values(num_points: usize) -> Vec<Value> {
    (0..num_points)
        .map(|i| Value::Number((i as i64 * 10).into()))
        .collect()
}

/// Generates text values for indexing benchmarks
pub fn generate_text_values(num_points: usize) -> Vec<Value> {
    (0..num_points)
        .map(|i| Value::String(format!("foo bar {}", i)))
        .collect()
}
