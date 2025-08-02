use crate::storage::segment::{Point, PointId};
use serde_json::Value;
use sled::Db;
use std::collections::HashMap;

/// Converts the number into a big-endian value which is suitable for querying/storing in sled
fn encoded_number(n: u64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

pub struct NumericIndex(sled::Tree);
impl NumericIndex {
    pub fn open(db: &Db, name: &str) -> Self {
        let tree = db
            .open_tree(format!("{name}_numeric_index"))
            .expect("Failed to open sled tree");
        Self(tree)
    }

    pub fn upsert(&self, point_id: u64, value: i64) -> Result<(), sled::Error> {
        // In numeric tree, the point value becomes tree's key, and the point ID is part of a list of values.
        let tree_key = encoded_number(value as u64);

        // Fetch existing point IDs for this value
        let mut point_ids: Vec<u64> = self
            .0
            .get(&tree_key)?
            .map(|data| {
                let (decoded, cnt) =
                    bincode::decode_from_slice(&data, bincode::config::standard()).unwrap();
                println!("Decoded point IDs: {:?} with count {}", decoded, cnt);
                decoded
            })
            .unwrap_or_default();

        // Add point and sort
        point_ids.push(point_id);
        point_ids.sort();

        // Store back
        let serialized_ids = bincode::encode_to_vec(point_ids, bincode::config::standard())
            .map_err(|e| {
                sled::Error::Io(std::io::Error::other(format!("Failed to encode: {e}")))
            })?;

        self.0.insert(&tree_key, serialized_ids).map_err(|e| {
            sled::Error::Io(std::io::Error::other(format!(
                "Failed to insert into index tree: {e}"
            )))
        })?;

        Ok(())
    }

    pub fn query_gte(&self, value: i64) -> Result<Vec<PointId>, sled::Error> {
        let min_key = encoded_number(value as u64);

        let mut results = Vec::new();
        for item in self.0.range(min_key..) {
            let (_gte_value, encoded_point_ids) = item?;
            let point_ids: Vec<u64> =
                bincode::decode_from_slice(&encoded_point_ids, bincode::config::standard())
                    .map_err(|e| {
                        sled::Error::Io(std::io::Error::other(format!("Failed to decode: {e}")))
                    })?
                    .0;
            results.extend_from_slice(&point_ids);
        }

        Ok(results.into_iter().map(PointId::Id).collect())
    }
}

pub enum FieldIndex {
    Numeric(NumericIndex),
    Null,
}

impl FieldIndex {
    pub fn numeric(db: &Db, name: &str) -> Self {
        FieldIndex::Numeric(NumericIndex::open(db, name))
    }

    pub fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error> {
        match self {
            FieldIndex::Numeric(index) => match value {
                Value::Number(num) if num.is_i64() => {
                    let num_value = num.as_i64().unwrap();
                    index.upsert(point_id, num_value)
                }
                _ => Err(sled::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Not a valid i64 value",
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
    // ToDo: Take this from the collection config. Not default value
    pub fn default(db: &Db) -> Self {
        let indices = HashMap::from_iter(vec![(
            "price".to_string(),
            FieldIndex::numeric(db, "price"),
        )]);

        Self { indices }
    }

    pub fn get_index_names(&self) -> Vec<&str> {
        self.indices.keys().map(|k| k.as_str()).collect()
    }

    pub fn load(db: &Db) -> Self {
        // ToDo: Load indices from the database
        Self::default(db)
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
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_payload_index() {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db = sled::open(&tmp_dir).expect("Failed to open sled database");
        let index = PayloadIndex::default(&db);

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

        if let FieldIndex::Numeric(numeric_index) = field_index {
            let results = numeric_index.query_gte(40).unwrap();
            assert_eq!(results.len(), 6); // Points with ids 4, 5, 6, 7, 8, 9
            assert_eq!(results, (4..10).map(PointId::Id).collect::<Vec<_>>());
        } else {
            panic!("Expected NumericIndex");
        }
    }
}
