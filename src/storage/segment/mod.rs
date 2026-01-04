pub mod index;
pub mod point;

use crate::{
    api::points::Query,
    error::{StorageError, StorageResult},
    storage::index::payload_index::{IndexConfig, PayloadIndex},
};
use futures::StreamExt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

// re-export point imports
pub use point::{Point, PointId};
// todo: Introduce type alias InnerPointId and use everywhere instead of u64

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, vector storage?
    // ID tracker is valuable for building immutable segments, so same point can exist in multiple segments while only the latest version is visible
    pub payload_index: PayloadIndex,
    pub indexing_queue: Mutex<Vec<PointId>>,
    pub shutdown_flag: Arc<AtomicBool>,
}

impl Segment {
    pub fn create(
        segments_dir: &Path,
        payload_schema: BTreeMap<String, IndexConfig>,
    ) -> Result<Self, StorageError> {
        // ToDo: Have uuid segment ID
        let path = segments_dir.join("0");
        std::fs::create_dir_all(&path).expect("Failed to create segment directory");

        let db = sled::open(&path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to open segment database: {e}"))
        })?;

        let mut payload_index = PayloadIndex::get_or_create(&db)?;

        for (index_name, index_config) in payload_schema {
            payload_index
                .add_index(&db, &index_name, index_config)
                .map_err(|e| StorageError::ServiceError(format!("Failed to add index: {e}")))?;
        }

        Ok(Self {
            path,
            db,
            payload_index,
            indexing_queue: Mutex::new(Vec::new()),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn load(path: &PathBuf) -> StorageResult<Self> {
        if !path.exists() {
            return Err(StorageError::ServiceError(format!(
                "Segment path does not exist: {path:?}"
            )));
        }

        let db = sled::open(path).expect("Failed to open segment database");
        let payload_index = PayloadIndex::get_or_create(&db)?;

        Ok(Self {
            path: path.to_owned(),
            db,
            payload_index,
            indexing_queue: Mutex::new(Vec::new()),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Insert a batch of points into the segment
    pub fn insert_points(&self, points: &[Point]) -> StorageResult<()> {
        for point in points {
            let key = point.id.encode()?;
            let value = point.encode_payload()?;
            self.db.insert(key, value).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert point into segment db: {e}"))
            })?;
        }

        let point_ids = points
            .iter()
            .map(|point| point.id.clone())
            .collect::<Vec<_>>();
        self.queue_for_indexing(point_ids)?;

        self.db
            .flush()
            .map_err(|e| StorageError::ServiceError(format!("Failed to flush segment db: {e}")))?;
        Ok(())
    }

    /// Send some points to be indexed to the indexing queue
    /// It takes a lock, so it's better to call it for a batch of points at once
    pub fn queue_for_indexing(&self, point_ids: Vec<PointId>) -> StorageResult<()> {
        // todo: Should lock in smaller chunks to avoid long locks?
        let mut guard = self.indexing_queue.lock().map_err(|e| {
            StorageError::ServiceError(format!("Failed to lock indexing queue: {}", e))
        })?;
        guard.extend(point_ids);
        Ok(())
    }

    pub async fn get_points(&self, ids: Option<Vec<PointId>>) -> Result<Vec<Point>, StorageError> {
        match ids {
            None => {
                // This iteration will block the thread since sled iter is sequential
                // So we should move it to a blocking context so it doesn't block the event loop
                let db_clone = self.db.clone();
                let points =
                    tokio::task::spawn_blocking(move || -> Result<Vec<Point>, StorageError> {
                        let points = db_clone
                            .iter()
                            .map(|result| match result {
                                Ok((key, value)) => Ok(Point::decode(&key, &value)?),
                                Err(e) => Err(StorageError::ServiceError(format!(
                                    "Failed to iterate over segment db: {e}"
                                ))),
                            })
                            .collect::<Result<Vec<Point>, StorageError>>()?;
                        Ok(points)
                    })
                    .await
                    .map_err(|e| {
                        StorageError::ServiceError(format!("Failed to join task: {e}"))
                    })??;

                Ok(points)
            }
            Some(ids) if ids.is_empty() => Ok(Vec::new()),
            Some(ids) => {
                // If we have a single item to read, we don't need to add overhead of chunking and parallelizing
                if ids.len() == 1 {
                    let key = ids[0].encode()?;
                    // db.get is a blocking call in async runtime, but it's only a single item so it should be okay
                    // todo: can be optimized later when we have io_uring?
                    if let Some(value) = self.db.get(&key)? {
                        return Ok(vec![Point::decode(&key, &value)?]);
                    }
                }

                // Othewise, chunk ids and fetch in parallel threads
                const MAX_CONCURRENT: usize = 10;
                const CHUNK_SIZE: usize = 50; // tune as appropriate

                // Convert chunks to owned vectors to satisfy 'static lifetime requirement of tokio::task::spawn_blocking
                let chunks: Vec<Vec<PointId>> =
                    ids.chunks(CHUNK_SIZE).map(|chunk| chunk.to_vec()).collect();

                // Each chunk will read 50 points and we will read 10 chunks in parallel => 500 points in parallel
                let point_chunks = futures::stream::iter(chunks.into_iter().map(|chunk| {
                    let db = self.db.clone();
                    tokio::task::spawn_blocking(move || -> Result<Vec<Point>, StorageError> {
                        let mut found_points = Vec::with_capacity(chunk.len());
                        for id in chunk {
                            let key = id.encode()?;
                            if let Some(value) = db.get(&key)? {
                                found_points.push(Point::decode(&key, &value)?);
                            }
                        }
                        Ok(found_points)
                    })
                }))
                .buffer_unordered(MAX_CONCURRENT)
                .collect::<Vec<_>>()
                .await;

                let mut all_points = Vec::new();
                for res in point_chunks {
                    let points = res.map_err(|e| {
                        StorageError::ServiceError(format!("Failed to join task: {e}"))
                    })??;
                    all_points.extend(points);
                }
                Ok(all_points)
            }
        }
    }

    pub async fn query_points(&self, query: Query) -> Result<Vec<Point>, StorageError> {
        let point_ids = self.payload_index.query(query).map_err(|e| {
            StorageError::ServiceError(format!("Failed to query payload index: {e}"))
        })?;
        let points = self.get_points(Some(point_ids)).await?;

        Ok(points)
    }

    pub fn count_points(&self) -> usize {
        self.db.len()
    }

    /// Get the current indexing queue length
    pub fn indexing_queue_length(&self) -> StorageResult<usize> {
        let queue = self.indexing_queue.lock().map_err(|e| {
            StorageError::ServiceError(format!("Failed to lock indexing queue: {e}"))
        })?;
        Ok(queue.len())
    }

    /// Signals the indexing loop to shutdown
    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Release);
    }

    /// Runs the background indexing loop, batching points efficiently.
    pub async fn run_indexing_loop(&self) -> StorageResult<()> {
        const INDEXING_THRESHOLD: usize = 100;
        const INDEXING_INTERVAL_MS: u64 = 100;
        const SLEEP_MS: u64 = 10;

        let mut point_ids = Vec::with_capacity(INDEXING_THRESHOLD);
        let mut last_index_time = Instant::now();

        loop {
            // Check shutdown flag
            if self.shutdown_flag.load(Ordering::Acquire) {
                log::info!("Indexing loop shutting down");
                break;
            }

            // Fill the batch up to threshold or until interval passes.
            while point_ids.len() < INDEXING_THRESHOLD
                && last_index_time.elapsed().as_millis() < INDEXING_INTERVAL_MS as u128
            {
                // Check shutdown flag during batching
                if self.shutdown_flag.load(Ordering::Acquire) {
                    break;
                }

                let point_id_opt = {
                    let mut queue = self.indexing_queue.lock().map_err(|e| {
                        StorageError::ServiceError(format!(
                            "Failed to acquire lock on indexing queue: {e}"
                        ))
                    })?;
                    // todo: Should pop from the front. I tried using VecDeque instead of Vec.
                    // But it was slower. Need to investigate why or find alt.
                    queue.pop()
                };

                if let Some(point_id) = point_id_opt {
                    point_ids.push(point_id);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(SLEEP_MS));
                }
            }

            if !point_ids.is_empty() {
                log::info!(
                    "Indexing {} points. First point ID: {:?}",
                    point_ids.len(),
                    point_ids[0]
                );
                let points = self.get_points(Some(point_ids.clone())).await?;
                self.payload_index.insert_batch(&points)?;
                point_ids.clear();
            }

            last_index_time = Instant::now();
        }

        Ok(())
    }

    // todo: Allow updating payload index schema on the fly
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn test_segment_insert_read_points() {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let segment = Segment::create(tmp_dir.path(), BTreeMap::new()).unwrap();
        let p1 = Point {
            id: PointId::Id(1),
            payload: json!({ "price": 100 }),
        };
        let p2 = Point {
            id: PointId::Id(2),
            payload: json!({ "price": 200 }),
        };
        segment.insert_points(&[p1.clone(), p2.clone()]).unwrap();

        // Read single point:
        let single_point = segment
            .get_points(Some(vec![PointId::Id(1)]))
            .await
            .unwrap();
        assert_eq!(single_point, vec![p1.clone()]);

        // Read all points:
        let all_points = segment.get_points(None).await.unwrap();
        assert_eq!(all_points, vec![p1, p2]);
    }
}
