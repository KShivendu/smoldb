pub mod index;
pub mod point;

use crate::{
    api::points::Query,
    error::StorageError,
    storage::index::payload_index::{IndexConfig, PayloadIndex},
};
use futures::StreamExt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

// re-export point imports
pub use point::{Point, PointId};

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, vector storage?
    // ID tracker is valuable for building immutable segments, so same point can exist in multiple segments while only the latest version is visible
    pub payload_index: PayloadIndex,
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

        let mut payload_index = PayloadIndex::get_or_create(&db);

        for (index_name, index_config) in payload_schema {
            payload_index
                .add_index(&db, &index_name, index_config)
                .map_err(|e| StorageError::ServiceError(format!("Failed to add index: {e}")))?;
        }

        Ok(Self {
            path,
            db,
            payload_index,
        })
    }

    pub fn load(path: &PathBuf) -> Result<Self, StorageError> {
        if !path.exists() {
            return Err(StorageError::ServiceError(format!(
                "Segment path does not exist: {path:?}"
            )));
        }

        let db = sled::open(path).expect("Failed to open segment database");
        let payload_index = PayloadIndex::get_or_create(&db);

        Ok(Self {
            path: path.to_owned(),
            db,
            payload_index,
        })
    }

    /// Insert a batch of points into the segment
    pub fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        for point in points {
            let key = point.id.into_string();
            let value = point.encode()?;
            self.db.insert(key, value).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert point into segment db: {e}"))
            })?;

            self.payload_index.upsert(point).map_err(|e| {
                StorageError::ServiceError(format!("Failed to update payload index: {e}"))
            })?;
        }

        self.db
            .flush()
            .map_err(|e| StorageError::ServiceError(format!("Failed to flush segment db: {e}")))?;
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
                                Ok((_point_id, value)) => Ok(Point::decode(&value)?),
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
                    let key = ids[0].into_string();
                    // db.get is a blocking call in async runtime, but it's only a single item so it should be okay
                    // todo: can be optimized later when we have io_uring?
                    if let Some(value) = self.db.get(key)? {
                        return Ok(vec![Point::decode(&value)?]);
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
                            let key = id.into_string();
                            if let Some(value) = db.get(key)? {
                                let point = Point::decode(&value)?;
                                found_points.push(point);
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
