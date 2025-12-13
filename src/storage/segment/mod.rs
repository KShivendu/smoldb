pub mod index;
pub mod point;

use crate::{
    api::points::Query,
    error::StorageError,
    storage::index::payload_index::{IndexConfig, PayloadIndex},
};
use futures::{stream, StreamExt, TryStreamExt};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

// re-export point imports
pub use point::{Point, PointId};

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, data storage, etc
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
            let value = point.encode().map_err(|e| {
                StorageError::ServiceError(format!("Failed to serialize point: {e}"))
            })?;
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
        let mut points = Vec::new();

        let Some(ids) = ids else {
            // If no ids are provided, return all points
            for result in self.db.iter() {
                match result {
                    Ok((_point_id, value)) => {
                        let point: Point = Point::decode(&value)?;
                        points.push(point);
                    }
                    Err(e) => {
                        return Err(StorageError::ServiceError(format!(
                            "Failed to iterate over segment db: {e}"
                        )))
                    }
                }
            }
            return Ok(points);
        };

        // If ids are provided, read only those points
        // We need to read them in parallel to speed up (esp since its reading from disk from random locations)
        let db_inner = self.db.clone(); // sled Db is thread-safe
        const MAX_CONCURRENT: usize = 10; // If you have too many concurrent tasks, it can hurt performance

        let points: Vec<Point> = stream::iter(ids)
            .map(|id| {
                let db = db_inner.clone();
                async move {
                    tokio::task::spawn_blocking(move || -> Result<Option<Point>, StorageError> {
                        let key = id.into_string(); // this is taking small time
                        if let Some(value) = db.get(key)? {
                            // this takes insane amount of time
                            let point: Point = Point::decode(&value)?;
                            Ok(Some(point))
                        } else {
                            Ok(None)
                        }
                    })
                    .await
                    .map_err(|e| StorageError::ServiceError(format!("Failed to join task: {e}")))?
                }
            })
            .buffer_unordered(MAX_CONCURRENT)
            .try_filter_map(|result| async move {
                match result {
                    Some(point) => Ok(Some(point)),
                    None => Ok(None), // Point not found, filter out
                }
            })
            .try_collect()
            .await?;

        Ok(points)
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
