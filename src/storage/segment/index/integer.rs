use serde_json::Value;
use sled::Db;
use std::ops::Bound;

use crate::storage::{
    index::{
        filter::FilterOperator,
        payload_index::{decoded_point_ids, encoded_integer_value, encoded_point_ids},
    },
    segment::PointId,
};

pub struct IntegerIndex(sled::Tree);

impl IntegerIndex {
    pub fn open(db: &Db, name: &str) -> Self {
        let tree = db
            .open_tree(format!("{name}_numeric_index"))
            .expect("Failed to open sled tree");
        Self(tree)
    }

    pub fn upsert(&self, point_id: u64, value: i64) -> Result<(), sled::Error> {
        // In numeric tree, the point value becomes tree's key, and the point ID is part of a list of values.
        let tree_key = encoded_integer_value(value);

        // Fetch existing point IDs for this value
        let mut point_ids: Vec<u64> = self
            .0
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
        let encoded_ids = encoded_point_ids(&point_ids).map_err(|e| {
            sled::Error::Io(std::io::Error::other(format!(
                "Failed to encode point IDs: {e}"
            )))
        })?;

        self.0.insert(&tree_key, encoded_ids).map_err(|e| {
            sled::Error::Io(std::io::Error::other(format!(
                "Failed to insert into index tree: {e}"
            )))
        })?;

        Ok(())
    }

    pub fn query(
        &self,
        value: i64,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> Result<Vec<PointId>, sled::Error> {
        let mut results = Vec::new();

        let value = encoded_integer_value(value);

        let bounds = match operation {
            FilterOperator::Gte => (Bound::Included(value), Bound::Unbounded),
            FilterOperator::Gt => (Bound::Excluded(value), Bound::Unbounded),
            FilterOperator::Lt => (Bound::Unbounded, Bound::Excluded(value)),
            FilterOperator::Lte => (Bound::Unbounded, Bound::Included(value)),
            FilterOperator::Eq => (Bound::Included(value.clone()), Bound::Included(value)),
        };

        for item in self.0.range(bounds) {
            let (_gte_value, encoded_point_ids) = item?;
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

    pub fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error> {
        match value {
            Value::Number(num) if num.is_i64() => {
                let num_value = num.as_i64().unwrap();
                self.upsert(point_id, num_value)
            }
            _ => Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{value} is not a valid i64 value"),
            ))),
        }
    }
}
