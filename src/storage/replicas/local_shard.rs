use crate::{
    storage::{
        error::{CollectionError, CollectionResult, StorageError},
        replicas::ShardOperationTrait,
        segment::{Point, PointId, Segment},
    },
    types::{SegmentId, ShardId},
};
use std::{collections::HashMap, path::PathBuf};
use tonic::async_trait;

const SEGMENTS_DIR: &str = "segments";

pub struct LocalShard {
    pub id: ShardId,
    pub path: PathBuf,
    pub segments: HashMap<SegmentId, Segment>,
    // ToDo: Wal
}

#[async_trait]
impl ShardOperationTrait for LocalShard {
    async fn get_points(&self, ids: Option<Vec<PointId>>) -> CollectionResult<Vec<Point>> {
        if let Some(segment) = self.segments.get(&0) {
            segment.get_points(ids).map_err(|e| {
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
            .ok_or(StorageError::ServiceError(
                "Couldn't parse shard id from shard directory".to_string(),
            ))?;

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

    pub fn count_points(&self) -> usize {
        if let Some(segment) = self.segments.get(&0) {
            segment.count_points()
        } else {
            0
        }
    }
}
