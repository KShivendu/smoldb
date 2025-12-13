use std::{collections::BTreeMap, sync::Arc};

use serde_json::json;
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
            payload: json!({ "msg": format!("Hello world {}", id), "price": id as i64 * 10 }),
        })
        .collect()
}

/// Creates a collection and initializes it with the given points
pub async fn create_collection_with_points(
    tempdir: &TempDir,
    channel_service: Arc<ChannelService>,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
    points: Vec<Point>,
) -> Collection {
    let collection =
        create_collection("test_collection", tempdir, channel_service, payload_schema).await;
    collection.upsert_points(points, true).await.unwrap();
    collection
}
