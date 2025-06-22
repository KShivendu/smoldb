use std::{collections::HashMap, path::PathBuf};

use crate::storage::error::StorageError;

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

        LocalShard {
            id,
            path: path.to_owned(),
            segments: HashMap::from_iter([(0, Segment::create(&segments_dir))]),
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

        Ok(Self {
            id,
            path: path.to_owned(),
            segments: segment_paths
                .into_iter()
                .map(|(id, path)| (id, Segment::load(&path)))
                .collect(),
        })
    }
}

pub struct Segment {
    pub path: PathBuf,
    // ToDo: ID tracker, data storage, index, etc
}

impl Segment {
    pub fn create(segments_dir: &PathBuf) -> Self {
        // ToDo: Have uuid segment ID
        let path = segments_dir.join("0");
        std::fs::create_dir_all(&path).expect("Failed to create segment directory");

        Self { path }
    }

    pub fn load(path: &PathBuf) -> Self {
        if !path.exists() {
            panic!("Segment path does not exist: {:?}", path);
        }

        Self {
            path: path.to_owned(),
        }
    }
}
