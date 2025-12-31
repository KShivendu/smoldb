use crate::{
    api::points::Query,
    error::{StorageError, StorageResult},
    storage::{
        index::{filter::FilterOperator, integer::IntegerIndex, text::TextIndex},
        segment::{Point, PointId},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sled::Db;
use std::collections::BTreeMap;
use utoipa::ToSchema;

/// Converts the number into a big-endian value which is suitable for querying/storing in sled
/// This allows lexicographical ordering and hence numeric comparisons
pub fn encoded_integer_value(n: i64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

/// Reverse of `encoded_integer_value`
pub fn decoded_integer_value(data: &[u8]) -> i64 {
    let mut buf = [0u8; 8]; // 8 bytes for i64
    buf.copy_from_slice(data);
    i64::from_be_bytes(buf)
}

pub fn encoded_point_ids(point_ids: &[u64]) -> StorageResult<Vec<u8>> {
    bincode::encode_to_vec(point_ids, bincode::config::standard())
        .map_err(|e| StorageError::CodecError(format!("Failed to encode point IDs: {e}")))
}

pub fn decoded_point_ids(data: &[u8]) -> StorageResult<Vec<u64>> {
    bincode::decode_from_slice(data, bincode::config::standard())
        .map(|(ids, _)| ids)
        .map_err(|e| StorageError::CodecError(format!("Failed to decode point IDs: {e}")))
}

pub enum FieldIndex {
    Int(IntegerIndex),
    Text(TextIndex),
    Null,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum IndexConfig {
    Int,
    Text,
    Null,
}

impl FieldIndex {
    pub fn new_numeric(db: &Db, name: &str) -> StorageResult<Self> {
        let index = IntegerIndex::open(db, name, true)?;
        Ok(FieldIndex::Int(index))
    }

    pub fn new_text(db: &Db, name: &str) -> StorageResult<Self> {
        let index = TextIndex::open(db, name, true)?;
        Ok(FieldIndex::Text(index))
    }
}

impl FieldIndexTrait<&Value> for FieldIndex {
    fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()> {
        match self {
            FieldIndex::Int(index) => index.add_point(point_id, value),
            FieldIndex::Text(index) => index.add_point(point_id, value),
            FieldIndex::Null => Err(StorageError::BadInput(
                "Null index is not supported yet".to_string(),
            )),
        }
    }

    fn open(_db: &Db, _name: &str, _use_in_memory: bool) -> StorageResult<Self> {
        Err(StorageError::BadInput(
            "Use specific index constructors like new_numeric or new_text".to_string(),
        ))
    }

    fn query(
        &self,
        value: &Value,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        let results = match self {
            FieldIndex::Int(int_index) => {
                let value = value.as_i64().ok_or_else(|| {
                    StorageError::BadInput(format!(
                        "Integer index query value must be an integer. Found {value}"
                    ))
                })?;

                int_index.query(value, operation, limit)?
            }
            FieldIndex::Null => {
                return Err(StorageError::BadInput(
                    "Null index queries are not supported yet".to_string(),
                ));
            }
            FieldIndex::Text(text_index) => {
                let value = value.as_str().ok_or_else(|| {
                    StorageError::BadInput(format!(
                        "Text index query value must be a string. Found {value}"
                    ))
                })?;

                text_index.query(value, &FilterOperator::Eq, limit)?
            }
        };

        Ok(results)
    }
}

pub struct PayloadIndex {
    pub indices: BTreeMap<String, FieldIndex>,
}

impl PayloadIndex {
    /// Read from the database to get existing indices and their types. If no indices are found, create a new empty index.
    pub fn get_or_create(db: &Db) -> StorageResult<Self> {
        let schema_tree = db
            .open_tree("schema")
            .expect("Failed to open segment schema tree");
        let mut indices = BTreeMap::new();
        for item in schema_tree.iter() {
            let (key, value) = item.expect("Failed to read schema item");
            let name = String::from_utf8(key.to_vec()).expect("Invalid UTF-8 in key");
            let index_config: IndexConfig =
                serde_json::from_slice(&value).expect("Failed to deserialize index config");
            let field_index = match index_config {
                IndexConfig::Int => FieldIndex::new_numeric(db, &name),
                IndexConfig::Null => {
                    return Err(StorageError::BadInput(
                        "Null index is not implemented yet".to_string(),
                    ))
                }
                IndexConfig::Text => FieldIndex::new_text(db, &name),
            }?;
            indices.insert(name, field_index);
        }

        Ok(Self { indices })
    }

    pub fn get_index_names(&self) -> Vec<&str> {
        self.indices.keys().map(|k| k.as_str()).collect()
    }

    pub fn add_index(
        &mut self,
        db: &Db,
        name: &str,
        index_config: IndexConfig,
    ) -> StorageResult<()> {
        if self.indices.contains_key(name) {
            return Err(StorageError::BadInput(format!(
                "Index with name '{name}' already exists",
            )));
        }

        let index = match index_config {
            IndexConfig::Int => FieldIndex::new_numeric(db, name)?,
            IndexConfig::Null => {
                return Err(StorageError::BadInput(
                    "Null index is not implemented yet".to_string(),
                ))
            }
            IndexConfig::Text => FieldIndex::new_text(db, name)?,
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
    // todo: Use id tracker so that we can support different PointId types while being storage efficient.
    pub fn upsert(&self, point: &Point) -> StorageResult<()> {
        let PointId::Id(point_id) = point.id else {
            return Err(StorageError::BadInput("Invalid PointId type: Only u64 point ID is supported for payload indexing (for now)".to_string()));
        };

        for (index_key, index_tree) in &self.indices {
            if let Some(payload_value) = point.payload.get(index_key) {
                index_tree.add_point(point_id, payload_value)?;
            }
        }
        Ok(())
    }

    // Todo: Support deleting points from the index in case of update or deletes

    pub fn query(&self, query: Query) -> StorageResult<Vec<PointId>> {
        // ToDo: Should allow querying for un-indexed fields to demonstrate/benchmark difference
        let index = self.indices.get(&query.filter.key).ok_or_else(|| {
            StorageError::BadInput(format!("No index found for key '{}'", query.filter.key))
        })?;

        // Todo: Support combining results from multiple indices for complex queries

        let results = index.query(&query.filter.value, &query.filter.op, query.limit)?;

        Ok(results)
    }
}

pub trait FieldIndexTrait<DataType> {
    /// Create or load an index from the DB
    ///
    /// Note for `use_in_memory`:
    /// Whether to query the on-disk index or the in-memory index
    /// If yes, writes will update both in-memory and on-disk index
    fn open(db: &Db, name: &str, use_in_memory: bool) -> StorageResult<Self>
    where
        Self: Sized;
    /// Add a point to the index
    fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()>;
    /// Query the index
    fn query(
        &self,
        value: DataType,
        operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>>;
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use crate::storage::index::filter::FilterOperator;

    use super::*;

    #[test]
    fn test_payload_index() -> StorageResult<()> {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db = sled::open(&tmp_dir).expect("Failed to open sled database");
        let mut index = PayloadIndex::get_or_create(&db)?;

        index.add_index(&db, "price", IndexConfig::Int)?;
        index.add_index(&db, "description", IndexConfig::Text)?;

        assert_eq!(index.get_index_names(), ["description", "price"]);

        for i in 0..10 {
            index.upsert(&Point {
                id: PointId::Id(i),
                payload: json!({"price": i * 10, "description": format!("foo {i}")}),
            })?;
        }

        // Todo: Test on-disk and in-memory index separately

        if let Some(FieldIndex::Int(int_index)) = index.indices.get("price") {
            let results = int_index.query(40, &FilterOperator::Gte, None)?;
            assert_eq!(results.len(), 6); // Points with ids 4, 5, 6, 7, 8, 9
            assert_eq!(results, (4..10).map(PointId::Id).collect::<Vec<_>>());
        } else {
            panic!("Expected NumericIndex");
        }

        if let Some(FieldIndex::Text(text_index)) = index.indices.get("description") {
            let results = text_index.query("4", &FilterOperator::Eq, None)?;
            assert_eq!(results.len(), 1);
            assert_eq!(results, vec![PointId::Id(4)]);

            let results = text_index.query("missingTerm", &FilterOperator::Eq, None)?;
            assert_eq!(results.len(), 0);
        } else {
            panic!("Expected TextIndex");
        }

        Ok(())
    }
}
