use crate::{error::StorageError, storage::index::payload_index::PayloadIndex};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[serde(untagged)]
pub enum PointId {
    Id(u64),
    Uuid(String),
}

impl PointId {
    pub fn into_string(&self) -> String {
        match self {
            PointId::Id(id) => id.to_string(),
            PointId::Uuid(uuid) => uuid.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Point {
    pub id: PointId,
    pub payload: serde_json::Value,
}

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, data storage, etc
    pub payload_index: PayloadIndex,
}

impl Segment {
    pub fn create(segments_dir: &Path) -> Result<Self, StorageError> {
        // ToDo: Have uuid segment ID
        let path = segments_dir.join("0");
        std::fs::create_dir_all(&path).expect("Failed to create segment directory");

        let db = sled::open(&path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to open segment database: {e}"))
        })?;

        let payload_index = PayloadIndex::get_or_create(&db);

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

    pub fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        for point in points {
            let key = point.id.into_string();
            let value = serde_json::to_string(point).map_err(|e| {
                StorageError::ServiceError(format!("Failed to serialize point: {e}"))
            })?;
            self.db.insert(key, value.as_str()).map_err(|e| {
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

    pub fn get_points(&self, ids: Option<Vec<PointId>>) -> Result<Vec<Point>, StorageError> {
        let mut points = Vec::new();

        let Some(ids) = ids else {
            // If no ids are provided, return all points
            for result in self.db.iter() {
                match result {
                    Ok((_, value)) => {
                        let point: Point = serde_json::from_slice(&value).map_err(|e| {
                            StorageError::ServiceError(format!("Failed to deserialize point: {e}"))
                        })?;
                        points.push(point);
                    }
                    Err(e) => {
                        return Err(StorageError::ServiceError(format!(
                            "Failed to iterate over segment db: {e}"
                        )));
                    }
                }
            }
            return Ok(points);
        };

        // If ids are provided, read only those points
        for id in ids {
            let key = id.into_string();
            if let Some(value) = self.db.get(key).map_err(|e| {
                StorageError::ServiceError(format!("Failed to get point from segment db: {e}"))
            })? {
                let point: Point = serde_json::from_slice(&value).map_err(|e| {
                    StorageError::ServiceError(format!("Failed to deserialize point: {e}"))
                })?;
                points.push(point);
            }
        }
        Ok(points)
    }

    pub fn count_points(&self) -> usize {
        self.db.len()
    }
}
