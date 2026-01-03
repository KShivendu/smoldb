use crate::{
    api::points::Query,
    error::{CollectionError, CollectionResult, StorageError},
    storage::{
        index::payload_index::IndexConfig,
        replicas::{ShardOperationTrait, ShardState},
        segment::{Point, PointId, Segment},
    },
    types::{SegmentId, ShardId},
};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    thread::JoinHandle,
};
use tonic::async_trait;

const SEGMENTS_DIR: &str = "segments";

pub struct LocalShard {
    pub id: ShardId,
    pub path: PathBuf,
    pub segments: HashMap<SegmentId, Arc<Segment>>,
    pub shard_state: ShardState,
    pub(crate) _indexing_threads: HashMap<SegmentId, JoinHandle<()>>,
    // ToDo: Wal
}

#[async_trait]
impl ShardOperationTrait for LocalShard {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>> {
        if let Some(segment) = self.segments.get(&0) {
            segment.get_points(ids).await.map_err(|e| {
                CollectionError::StorageError(StorageError::ServiceError(format!(
                    "Failed to get points from segment: {e}"
                )))
            })
        } else {
            Err(StorageError::ServiceError(
                "No segments available".to_string(),
            ))?
        }
    }

    async fn upsert_points(&self, points: Vec<Point>) -> CollectionResult<()> {
        // ToDo: Select segment based on point id or some other criteria
        if let Some(segment) = self.segments.get(&0) {
            segment.insert_points(&points)?;
            Ok(())
        } else {
            Err(CollectionError::ServiceError(
                "No segments available".to_string(),
            ))
        }
    }

    async fn query_points(&self, query: Query) -> CollectionResult<Vec<Point>> {
        if let Some(segment) = self.segments.get(&0) {
            segment.query_points(query).await.map_err(|e| {
                CollectionError::StorageError(StorageError::ServiceError(format!(
                    "Failed to query points from segment: {e}"
                )))
            })
        } else {
            Err(StorageError::ServiceError(
                "No segments available".to_string(),
            ))?
        }
    }
}

impl LocalShard {
    /// Spawns a dedicated thread to run the indexing loop for a segment
    fn spawn_indexing_thread(segment_id: SegmentId, segment: Arc<Segment>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for indexing thread");

            rt.block_on(async {
                if let Err(e) = segment.run_indexing_loop().await {
                    log::error!("Indexing loop error for segment {}: {}", segment_id, e); // for general logs
                    eprintln!("Indexing loop error for segment {}: {}", segment_id, e);
                    // for benches with --nocapture
                }
            });
        })
    }

    pub fn init(
        path: PathBuf,
        id: ShardId,
        payload_schema: Option<BTreeMap<String, IndexConfig>>,
    ) -> Self {
        let segments_dir = path.join(SEGMENTS_DIR);
        std::fs::create_dir_all(&segments_dir).expect("Failed to create segments directory");

        let payload_schema = payload_schema.unwrap_or_default();

        let segment0 = Segment::create(&segments_dir, payload_schema)
            .expect("Failed to create initial segment");

        let segment0_arc = Arc::new(segment0);
        let segment0_clone = segment0_arc.clone();
        let indexing_thread = Self::spawn_indexing_thread(0, segment0_clone);

        let mut segments = HashMap::new();
        segments.insert(0, segment0_arc);

        let mut indexing_threads = HashMap::new();
        indexing_threads.insert(0, indexing_thread);

        LocalShard {
            id,
            path: path.to_owned(),
            segments,
            shard_state: ShardState::Active,
            _indexing_threads: indexing_threads,
        }
    }

    pub fn load(path: &PathBuf) -> Result<Self, StorageError> {
        let segments_dir = path.join(SEGMENTS_DIR);
        std::fs::create_dir_all(&segments_dir).expect("Failed to create segments directory");

        let segment_paths = std::fs::read_dir(&segments_dir)
            .expect("Failed to read segments directory")
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    e.path().file_name().and_then(|name| {
                        name.to_str()
                            .and_then(|s| s.parse::<SegmentId>().ok())
                            .map(|id| (id, e.path()))
                    })
                })
            })
            .collect::<HashMap<SegmentId, PathBuf>>();

        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<ShardId>().ok())
            .ok_or(StorageError::ServiceError(
                "Couldn't parse shard id from shard directory".to_string(),
            ))?;

        let mut segments = HashMap::new();
        let mut indexing_threads = HashMap::new();

        for (segment_id, segment_path) in segment_paths {
            let segment = Segment::load(&segment_path)?;
            let segment_arc = Arc::new(segment);
            let segment_clone = segment_arc.clone();
            let indexing_thread = Self::spawn_indexing_thread(segment_id, segment_clone);

            indexing_threads.insert(segment_id, indexing_thread);
            segments.insert(segment_id, segment_arc);
        }

        Ok(Self {
            id,
            path: path.to_owned(),
            segments,
            shard_state: ShardState::Active, // ToDo: Load from disk?
            _indexing_threads: indexing_threads,
        })
    }

    pub fn count_points(&self) -> usize {
        if let Some(segment) = self.segments.get(&0) {
            segment.count_points()
        } else {
            0
        }
    }

    /// Get the total count of pending points in the indexing queue across all segments
    pub fn get_pending_indexing_count(&self) -> usize {
        self.segments
            .values()
            .filter_map(|segment| segment.indexing_queue_length().ok())
            .sum()
    }
}

impl Drop for LocalShard {
    fn drop(&mut self) {
        // Signal all segments to shutdown their indexing loops
        for segment in self.segments.values() {
            segment.shutdown();
        }

        // Join all indexing threads to ensure they complete
        // We need to take ownership of the threads, so we'll use a temporary HashMap
        let mut threads = std::mem::take(&mut self._indexing_threads);
        for (segment_id, handle) in threads.drain() {
            if let Err(e) = handle.join() {
                log::error!(
                    "Failed to join indexing thread for segment {}: {:?}",
                    segment_id,
                    e
                );
            } else {
                log::debug!(
                    "Successfully joined indexing thread for segment {}",
                    segment_id
                );
            }
        }
    }
}
