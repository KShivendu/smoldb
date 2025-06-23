use crate::{
    api::points::{Point, PointId},
    storage::error::StorageError,
};
use std::path::PathBuf;

pub struct Segment {
    pub path: PathBuf,
    pub db: sled::Db,
    // ToDo: ID tracker, data storage, index, etc
}

impl Segment {
    pub fn create(segments_dir: &PathBuf) -> Result<Self, StorageError> {
        // ToDo: Have uuid segment ID
        let path = segments_dir.join("0");
        std::fs::create_dir_all(&path).expect("Failed to create segment directory");

        let db = sled::open(&path).map_err(|e| {
            StorageError::ServiceError(format!("Failed to open segment database: {}", e))
        })?;

        db.insert("msg", "hello world").map_err(|e| {
            StorageError::ServiceError(format!("Failed to insert into segment db: {}", e))
        })?;

        Ok(Self { path, db })
    }

    pub fn load(path: &PathBuf) -> Result<Self, StorageError> {
        if !path.exists() {
            return Err(StorageError::ServiceError(format!(
                "Segment path does not exist: {:?}",
                path
            )));
        }

        let db = sled::open(&path).expect("Failed to open segment database");

        Ok(Self {
            path: path.to_owned(),
            db,
        })
    }

    pub fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        for point in points {
            let key = point.id.into_string();
            let value = serde_json::to_string(point).map_err(|e| {
                StorageError::ServiceError(format!("Failed to serialize point: {}", e))
            })?;
            self.db.insert(key, value.as_str()).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert point into segment db: {}", e))
            })?;
        }
        self.db.flush().map_err(|e| {
            StorageError::ServiceError(format!("Failed to flush segment db: {}", e))
        })?;
        Ok(())
    }

    pub fn get_points(&self, ids: &[PointId]) -> Result<Vec<Point>, StorageError> {
        let mut points = Vec::new();
        for id in ids {
            let key = id.into_string();
            if let Some(value) = self.db.get(key).map_err(|e| {
                StorageError::ServiceError(format!("Failed to get point from segment db: {}", e))
            })? {
                let point: Point = serde_json::from_slice(&value).map_err(|e| {
                    StorageError::ServiceError(format!("Failed to deserialize point: {}", e))
                })?;
                points.push(point);
            }
        }
        Ok(points)
    }
}
