use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, Criterion};
use futures::executor::block_on;
use serde_json::json;
use smoldb::storage::index::payload_index::IndexConfig;
use smoldb::{
    channel_service::ChannelService,
    storage::{
        collection::{Collection, CollectionConfig},
        segment::{Point, PointId, Segment},
    },
};
use std::{
    collections::BTreeMap,
    fs::File,
    sync::{Arc, Once},
    time::Duration,
};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use tracing::info_span;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

const TEXT_FIELD: &str = "description";
const INT_FIELD: &str = "price";

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        // Chrome tracing layer for visual flame charts
        let (chrome_layer, guard) = ChromeLayerBuilder::new()
            .file("bench_trace.json")
            .include_args(true)
            .build();

        // File logging layer (existing)
        let file = File::create("bench_trace.log").unwrap();
        let fmt_layer = fmt::layer()
            .with_writer(file)
            .with_target(false)
            .with_ansi(false)
            .with_thread_ids(false)
            .with_level(false)
            .with_timer(fmt::time::uptime());

        tracing_subscriber::registry()
            .with(chrome_layer)
            .with(fmt_layer)
            .init();

        // Leak the guard so traces are flushed when the process exits
        std::mem::forget(guard);
    });
}

/// Creates a benchmark group with default configuration
pub fn benchmark_group<'a>(
    c: &'a mut Criterion,
    name: &str,
) -> BenchmarkGroup<'a, criterion::measurement::WallTime> {
    crate::setup_tracing();
    let mut group = c.benchmark_group(name);
    group.sample_size(100); // default is 100
    group.measurement_time(Duration::from_secs(5)); // default is 5s
    group.warm_up_time(Duration::from_secs(3)); // default is 3s
    group.significance_level(0.05);
    group.noise_threshold(0.05);
    group
}

/// Creates a temporary directory for benchmark storage
pub fn create_tempdir() -> TempDir {
    TempDir::new().expect("Failed to create temporary directory")
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

pub fn create_channel_service() -> Arc<ChannelService> {
    Arc::new(ChannelService::default())
}

pub fn create_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub async fn create_collection(
    name: &str,
    tempdir: &TempDir,
    channel_service: Arc<ChannelService>,
    payload_schema: Option<BTreeMap<String, IndexConfig>>,
) -> Collection {
    let config = CollectionConfig {
        params: "test_params".to_string(),
        payload_schema,
    };
    Collection::init(name.to_string(), config, tempdir.path(), channel_service)
        .await
        .unwrap()
}

// Takes 9.0491 µs on my machine
pub fn segment_write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "segment");

    {
        group.bench_function("write/single", |b| {
            b.iter_batched(
                || {
                    let tempdir = create_tempdir();
                    let segment = Segment::create(tempdir.path(), BTreeMap::new()).unwrap();
                    let points = generate_points(1);
                    (segment, points)
                },
                |(segment, points)| {
                    let _span = info_span!("segment single write").entered();
                    segment.insert_points(&points).unwrap();
                },
                BatchSize::LargeInput,
            );
        });
    }
}

pub fn collection_write(c: &mut Criterion) {
    let mut group = benchmark_group(c, "collection");

    {
        let rt = create_runtime();
        group.bench_function("write/single", |b| {
            b.to_async(&rt).iter_batched(
                || {
                    let tempdir = create_tempdir();
                    let channel_service = create_channel_service();
                    let collection = block_on(async {
                        create_collection("test_collection", &tempdir, channel_service, None).await
                    });
                    let points = generate_points(1);
                    (collection, points)
                },
                |(collection, points)| async move {
                    let _span = info_span!("collection single write").entered();
                    collection.upsert_points(points, true).await.unwrap();
                },
                BatchSize::LargeInput,
            );
        });
    }
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = segment_write, collection_write
);
criterion_main!(benches);
