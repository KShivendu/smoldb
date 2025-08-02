use crate::{
    api::points::Query,
    storage::{
        index::filter::FilterOperator,
        segment::{Point, PointId},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sled::Db;
use std::{
    collections::{HashMap, HashSet},
    ops::Bound,
};

/// Converts the number into a big-endian value which is suitable for querying/storing in sled
/// This allows lexicographical ordering and hence numeric comparisons
fn encoded_integer_value(n: i64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

fn encoded_point_ids(point_ids: &[u64]) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::encode_to_vec(point_ids, bincode::config::standard())
}

fn decoded_point_ids(data: &[u8]) -> Result<Vec<u64>, bincode::error::DecodeError> {
    bincode::decode_from_slice(data, bincode::config::standard()).map(|(ids, _)| ids)
}

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
        }

        Ok(results.into_iter().map(PointId::Id).collect())
    }
}

pub enum FieldIndex {
    Int(IntegerIndex),
    Null,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum IndexConfig {
    Int,
    Null,
}

impl FieldIndex {
    pub fn numeric(db: &Db, name: &str) -> Self {
        FieldIndex::Int(IntegerIndex::open(db, name))
    }

    pub fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error> {
        match self {
            FieldIndex::Int(index) => match value {
                Value::Number(num) if num.is_i64() => {
                    let num_value = num.as_i64().unwrap();
                    index.upsert(point_id, num_value)
                }
                _ => Err(sled::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{value} is not a valid i64 value"),
                ))),
            },
            FieldIndex::Null => {
                unimplemented!("Null index is not implemented yet");
            }
        }
    }
}

pub struct PayloadIndex {
    pub indices: HashMap<String, FieldIndex>,
}

impl PayloadIndex {
    pub fn get_or_create(db: &Db) -> Self {
        // Read from the database to get existing indices and their types:
        let schema_tree = db.open_tree("schema").expect("Failed to open schema tree");
        let index_configs: HashMap<String, String> = schema_tree
            .iter()
            .map(|item| {
                let (key, value) = item.expect("Failed to read schema item");
                // ToDo: Ensure that using utf8 will not cause problems
                let index_name = String::from_utf8(key.to_vec()).expect("Invalid UTF-8 in key");
                let index_config =
                    String::from_utf8(value.to_vec()).expect("Invalid UTF-8 in value");

                (index_name, index_config)
            })
            .collect();

        let mut indices = HashMap::new();
        for (name, index_config) in index_configs {
            let index_config: IndexConfig =
                serde_json::from_str(&index_config).expect("Failed to deserialize index config");
            match index_config {
                IndexConfig::Int => {
                    indices.insert(name.clone(), FieldIndex::numeric(db, &name));
                }
                IndexConfig::Null => {
                    indices.insert(name.clone(), FieldIndex::Null);
                }
            }
        }

        Self { indices }
    }

    pub fn get_index_names(&self) -> Vec<&str> {
        self.indices.keys().map(|k| k.as_str()).collect()
    }

    pub fn add_index(
        &mut self,
        db: &Db,
        name: &str,
        index_config: IndexConfig,
    ) -> Result<(), sled::Error> {
        if self.indices.contains_key(name) {
            return Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Index with name '{name}' already exists"),
            )));
        }

        let index = match index_config {
            IndexConfig::Int => FieldIndex::numeric(db, name),
            IndexConfig::Null => FieldIndex::Null,
        };

        let index_config_str =
            serde_json::to_string(&index_config).expect("Failed to serialize index config");

        self.indices.insert(name.to_string(), index);
        // Now add to db schema so it's persisted:

        let schema_tree = db.open_tree("schema").expect("Failed to open schema tree");
        schema_tree
            .insert(name.as_bytes(), index_config_str.as_bytes())
            .expect("Failed to insert into schema tree");

        Ok(())
    }

    pub fn upsert(&self, point: &Point) -> Result<(), sled::Error> {
        let PointId::Id(point_id) = point.id else {
            return Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid PointId type",
            )));
        };

        for (index_key, index_tree) in &self.indices {
            if let Some(payload_value) = point.payload.get(index_key) {
                index_tree.add_point(point_id, payload_value)?;
            }
        }
        Ok(())
    }

    pub fn query(&self, query: Query) -> Result<Vec<PointId>, sled::Error> {
        let mut results = HashSet::new();
        for (index_name, index) in &self.indices {
            if *index_name == query.filter.key {
                match index {
                    FieldIndex::Int(int_index) => {
                        let value = query.filter.value.parse::<i64>().unwrap();
                        let query_results = int_index.query(value, &query.filter.op)?;
                        results.extend(query_results);
                    }
                    FieldIndex::Null => {
                        unimplemented!("Null index queries are not implemented yet");
                    }
                }
            }
        }
        Ok(results.into_iter().collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_payload_index() {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db = sled::open(&tmp_dir).expect("Failed to open sled database");
        let mut index = PayloadIndex::get_or_create(&db);

        index.add_index(&db, "price", IndexConfig::Int).unwrap();

        assert!(index.get_index_names().len() == 1);
        assert!(index.indices.contains_key("price"));

        for i in 0..10 {
            index
                .upsert(&Point {
                    id: PointId::Id(i),
                    payload: serde_json::json!({"price": i * 10}),
                })
                .unwrap();
        }

        let field_index = index.indices.get("price").unwrap();

        if let FieldIndex::Int(numeric_index) = field_index {
            let results = numeric_index.query(40, &FilterOperator::Gte).unwrap();
            assert_eq!(results.len(), 6); // Points with ids 4, 5, 6, 7, 8, 9
            assert_eq!(results, (4..10).map(PointId::Id).collect::<Vec<_>>());
        } else {
            panic!("Expected NumericIndex");
        }
    }
}
