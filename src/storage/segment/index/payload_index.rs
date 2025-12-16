use crate::{
    api::points::Query,
    storage::{
        index::{filter::FilterOperator, integer::IntegerIndex, text::TextIndex},
        segment::{Point, PointId},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sled::Db;
use std::collections::HashMap;
use utoipa::ToSchema;

/// Converts the number into a big-endian value which is suitable for querying/storing in sled
/// This allows lexicographical ordering and hence numeric comparisons
pub fn encoded_integer_value(n: i64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

pub fn encoded_point_ids(point_ids: &[u64]) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::encode_to_vec(point_ids, bincode::config::standard())
}

pub fn decoded_point_ids(data: &[u8]) -> Result<Vec<u64>, bincode::error::DecodeError> {
    bincode::decode_from_slice(data, bincode::config::standard()).map(|(ids, _)| ids)
}

pub enum FieldIndex {
    Int(IntegerIndex),
    Null,
    Text(TextIndex),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum IndexConfig {
    Int,
    Null,
    Text,
}

impl FieldIndex {
    pub fn new_numeric(db: &Db, name: &str) -> Self {
        FieldIndex::Int(IntegerIndex::open(db, name))
    }

    pub fn new_text(db: &Db, name: &str) -> Self {
        FieldIndex::Text(TextIndex::open(db, name))
    }
}

impl FieldIndexTrait<&Value> for FieldIndex {
    fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error> {
        match self {
            FieldIndex::Int(index) => index.add_point(point_id, value),
            FieldIndex::Null => {
                unimplemented!("Null index is not implemented yet");
            }
            FieldIndex::Text(index) => index.add_point(point_id, value),
        }
    }

    fn open(_db: &Db, _name: &str) -> Self {
        unimplemented!("Use specific index constructors like new_numeric or new_text");
    }

    fn query(
        &self,
        value: &Value,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> Result<Vec<PointId>, sled::Error> {
        let results = match self {
            FieldIndex::Int(int_index) => {
                let value = value.as_i64().ok_or_else(|| {
                    sled::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Integer index query value must be an integer",
                    ))
                })?;

                int_index.query(value, operation, limit)?
            }
            FieldIndex::Null => {
                unimplemented!("Null index queries are not implemented yet");
            }
            FieldIndex::Text(t) => {
                let Some(value) = value.as_str() else {
                    return Err(sled::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Text index query value must be a string",
                    )));
                };

                t.query(value, &FilterOperator::Eq, limit)?
            }
        };

        Ok(results)
    }
}

pub struct PayloadIndex {
    pub indices: HashMap<String, FieldIndex>,
}

impl PayloadIndex {
    /// Read from the database to get existing indices and their types. If no indices are found, create a new empty index.
    pub fn get_or_create(db: &Db) -> Self {
        let schema_tree = db
            .open_tree("schema")
            .expect("Failed to open segment schema tree");
        let mut indices = HashMap::new();
        for item in schema_tree.iter() {
            let (key, value) = item.expect("Failed to read schema item");
            let name = String::from_utf8(key.to_vec()).expect("Invalid UTF-8 in key");
            let index_config: IndexConfig =
                serde_json::from_slice(&value).expect("Failed to deserialize index config");
            let field_index = match index_config {
                IndexConfig::Int => FieldIndex::new_numeric(db, &name),
                IndexConfig::Null => FieldIndex::Null,
                IndexConfig::Text => FieldIndex::new_text(db, &name),
            };
            indices.insert(name, field_index);
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
            IndexConfig::Int => FieldIndex::new_numeric(db, name),
            IndexConfig::Null => unimplemented!("Null index is not implemented yet"),
            IndexConfig::Text => FieldIndex::new_text(db, name),
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

    /// ToDo: Support updating existing points in the index. This would require removing old values and adding new ones.
    /// ToDo: Decoupling indexing from upserts. So that we can upsert fast and index in the background.
    pub fn upsert(&self, point: &Point) -> Result<(), sled::Error> {
        // todo: Use id tracker so that we can support different PointId types while being storage efficient.
        let PointId::Id(point_id) = point.id else {
            return Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid PointId type: Only u64 point ID is supported for payload indexing (for now)",
            )));
        };

        for (index_key, index_tree) in &self.indices {
            if let Some(payload_value) = point.payload.get(index_key) {
                index_tree.add_point(point_id, payload_value)?;
            }
        }
        Ok(())
    }

    // Todo: Support deleting points from the index in case of update or deletes

    pub fn query(&self, query: Query) -> Result<Vec<PointId>, sled::Error> {
        // ToDo: Should allow querying for un-indexed fields to demonstrate/benchmark difference
        let index = self.indices.get(&query.filter.key).ok_or_else(|| {
            sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No index found for key '{}'", query.filter.key),
            ))
        })?;

        // Todo: Support combining results from multiple indices for complex queries

        let results = index.query(
            &Value::String(query.filter.value),
            &query.filter.op,
            query.limit,
        )?;

        Ok(results)
    }
}

pub trait FieldIndexTrait<DataType> {
    /// Create or load an index from the DB
    fn open(db: &Db, name: &str) -> Self;
    /// Add a point to the index
    fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error>;
    /// Query the index
    fn query(
        &self,
        value: DataType,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> Result<Vec<PointId>, sled::Error>;
}

#[cfg(test)]
mod test {
    use serde_json::json;

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
                    payload: json!({"price": i * 10}),
                })
                .unwrap();
        }

        let field_index = index.indices.get("price").unwrap();

        if let FieldIndex::Int(numeric_index) = field_index {
            let results = numeric_index.query(40, &FilterOperator::Gte, None).unwrap();
            assert_eq!(results.len(), 6); // Points with ids 4, 5, 6, 7, 8, 9
            assert_eq!(results, (4..10).map(PointId::Id).collect::<Vec<_>>());
        } else {
            panic!("Expected NumericIndex");
        }
    }
}
