use std::{collections::HashMap, path::PathBuf};

use crate::{
    api::points::{Point, PointId},
    storage::{error::StorageError, segment::Segment},
};

pub type SegmentId = u64;
pub type ShardId = u64;

const SEGMENTS_DIR: &str = "segments";

pub struct LocalShard {
    pub id: ShardId,
    pub path: PathBuf,
    pub segments: HashMap<SegmentId, Segment>,
    // ToDo: Wal
}

impl LocalShard {
    pub fn init(path: PathBuf, id: ShardId) -> Self {
        let segments_dir = path.join(SEGMENTS_DIR);
        std::fs::create_dir_all(&segments_dir).expect("Failed to create segments directory");

        let segment0 = Segment::create(&segments_dir).expect("Failed to create initial segment");

        LocalShard {
            id,
            path: path.to_owned(),
            segments: HashMap::from_iter([(0, segment0)]),
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
            .ok_or(StorageError::ServiceError(format!(
                "Couldn't parse shard id from shard directory"
            )))?;

        let mut segments = HashMap::new();
        for (id, segment_path) in segment_paths {
            let segment = Segment::load(&segment_path)?;
            segments.insert(id, segment);
        }

        Ok(Self {
            id,
            path: path.to_owned(),
            segments,
        })
    }

    pub fn insert_points(&self, points: &[Point]) -> Result<(), StorageError> {
        // ToDo: Select segment based on point id or some other criteria
        if let Some(segment) = self.segments.get(&0) {
            segment.insert_points(points)?;
            Ok(())
        } else {
            Err(StorageError::ServiceError(
                "No segments available".to_string(),
            ))
        }
    }

    pub fn get_points(&self, ids: &[PointId]) -> Result<Vec<Point>, StorageError> {
        if let Some(segment) = self.segments.get(&0) {
            segment.get_points(ids)
        } else {
            Err(StorageError::ServiceError(
                "No segments available".to_string(),
            ))
        }
    }
}
