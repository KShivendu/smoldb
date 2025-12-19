use serde_json::Value;
use sled::Db;
use std::{collections::BTreeMap, ops::Bound, sync::RwLock};

use crate::error::{StorageError, StorageResult};
use crate::storage::{
    index::{
        filter::FilterOperator,
        payload_index::{
            decoded_integer_value, decoded_point_ids, encoded_integer_value, encoded_point_ids,
        },
    },
    segment::PointId,
};

pub struct IntegerIndex {
    // For persistent storage
    tree: sled::Tree,
    // /// In-memory index copy for fast lookups
    in_memory: Option<InMemoryIntegerIndex>,
}

impl IntegerIndex {
    /// Note for `use_in_memory`:
    /// Whether to query the on-disk index or the in-memory index
    /// If yes, writes will update both in-memory and on-disk index
    pub fn open(db: &Db, name: &str, use_in_memory: bool) -> StorageResult<Self> {
        let tree = db
            .open_tree(format!("{name}_numeric_index"))
            .expect("Failed to open sled tree");

        let in_memory = if use_in_memory {
            Some(InMemoryIntegerIndex::new(&tree)?)
        } else {
            None
        };

        Ok(Self { tree, in_memory })
    }

    /// todo: Upserting should also remove the point id from the index?
    /// these should be decoupled from upsert so we can batch them
    pub fn upsert(&self, point_id: u64, value: i64) -> StorageResult<()> {
        // In numeric tree, the point value becomes tree's key, and the point ID is part of a list of values.
        let tree_key = encoded_integer_value(value);

        // Fetch existing point IDs for this value
        let mut point_ids: Vec<u64> = self
            .tree
            .get(&tree_key)?
            .map(|data| {
                decoded_point_ids(&data)
                    .unwrap_or_else(|e| panic!("Failed to decode point IDs: {e}"))
            })
            .unwrap_or_default();

        // Add point and sort
        point_ids.push(point_id);
        point_ids.sort();

        // Store back
        let encoded_ids = encoded_point_ids(&point_ids)?;

        self.tree.insert(&tree_key, encoded_ids).map_err(|e| {
            StorageError::ServiceError(format!("Failed to insert into index tree: {e}"))
        })?;

        if let Some(in_memory) = &self.in_memory {
            in_memory.insert(value, point_ids)?;
        }

        Ok(())
    }

    pub fn query(
        &self,
        value: i64,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        if let Some(in_memory) = &self.in_memory {
            return in_memory.query(value, operation, limit);
        };

        let mut results = Vec::new();

        let value = encoded_integer_value(value);

        let bounds = match operation {
            FilterOperator::Gte => (Bound::Included(value), Bound::Unbounded),
            FilterOperator::Gt => (Bound::Excluded(value), Bound::Unbounded),
            FilterOperator::Lt => (Bound::Unbounded, Bound::Excluded(value)),
            FilterOperator::Lte => (Bound::Unbounded, Bound::Included(value)),
            FilterOperator::Eq => (Bound::Included(value.clone()), Bound::Included(value)),
        };

        for item in self.tree.range(bounds) {
            let (_integer_value, encoded_point_ids) = item?;
            let point_ids = decoded_point_ids(&encoded_point_ids).map_err(|e| {
                sled::Error::Io(std::io::Error::other(format!("Failed to decode: {e}")))
            })?;
            results.extend_from_slice(&point_ids);

            if let Some(limit) = limit {
                if results.len() >= limit {
                    results.truncate(limit);
                    break;
                }
            }
        }

        Ok(results.into_iter().map(PointId::Id).collect())
    }

    pub fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()> {
        match value {
            Value::Number(num) if num.is_i64() => {
                let num_value = num.as_i64().unwrap();
                self.upsert(point_id, num_value)
            }
            _ => Err(StorageError::BadInput(format!(
                "{value} is not a valid i64 value",
            ))),
        }
    }
}

struct InMemoryIntegerIndex {
    /// I can get rid of these locks if I introduce immutable segments
    index: RwLock<BTreeMap<i64, Vec<u64>>>,
}

impl InMemoryIntegerIndex {
    // Load in-memory index from disk
    pub fn new(tree: &sled::Tree) -> StorageResult<Self> {
        log::debug!("Loading in-memory integer index from disk");
        let mut index = BTreeMap::new();
        for item in tree.iter() {
            let (key, value) = item?;
            let integer_value = decoded_integer_value(&key);
            let point_ids = decoded_point_ids(&value)?;
            index.insert(integer_value, point_ids);
        }

        Ok(Self {
            index: RwLock::new(index),
        })
    }

    pub fn insert(&self, value: i64, point_ids: Vec<u64>) -> StorageResult<()> {
        self.index
            .write()
            .map_err(|e| {
                StorageError::ServiceError(format!("Failed to write to in-memory index: {e}"))
            })?
            .insert(value, point_ids);
        Ok(())
    }

    pub fn query(
        &self,
        value: i64,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        let index_guard = self.index.read().map_err(|e| {
            StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
        })?;

        // todo: Convert into a generic that works with any type
        let bounds = match operation {
            FilterOperator::Gte => (Bound::Included(value), Bound::Unbounded),
            FilterOperator::Gt => (Bound::Excluded(value), Bound::Unbounded),
            FilterOperator::Lt => (Bound::Unbounded, Bound::Excluded(value)),
            FilterOperator::Lte => (Bound::Unbounded, Bound::Included(value)),
            FilterOperator::Eq => (Bound::Included(value), Bound::Included(value)),
        };

        let mut results = Vec::new();

        for (_int_value, point_ids) in index_guard.range(bounds) {
            results.extend_from_slice(point_ids);

            if let Some(limit) = limit {
                if results.len() >= limit {
                    results.truncate(limit);
                    break;
                }
            }
        }

        Ok(results.into_iter().map(PointId::Id).collect())
    }
}
