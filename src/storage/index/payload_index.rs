use crate::storage::segment::{Point, PointId};
use serde_json::Value;
use sled::Db;
use std::collections::HashMap;

pub struct NumericIndex(sled::Tree);

impl NumericIndex {
    pub fn open(db: &Db, name: &str) -> Self {
        let tree = db
            .open_tree(format!("{name}_numeric_index"))
            .expect("Failed to open sled tree");
        Self(tree)
    }

    pub fn upsert(&self, point_id: u64, value: i64) -> Result<(), sled::Error> {
        let mut key_bytes = Vec::new();
        // key_bytes.write_u64(point_id).await.unwrap(); // avoid async write:
        key_bytes.extend_from_slice(&point_id.to_be_bytes());
        self.0
            .insert(key_bytes, &value.to_be_bytes())
            .map_err(|e| {
                sled::Error::Io(std::io::Error::other(format!(
                    "Failed to insert into index tree: {e}"
                )))
            })?;

        Ok(())
    }

    pub fn query_gte(&self, value: i64) -> Result<Vec<PointId>, sled::Error> {
        let mut min_key = Vec::new();
        min_key.extend_from_slice(&value.to_be_bytes());

        let mut results = Vec::new();
        // let mut item = self.0.range(min_key..);
        for item in self.0.range(min_key..) {
            let (_key, _value) = item?;
            // let point_id = u64::from_be_bytes(value.to_vec());
            let point_id = 0;
            results.push(PointId::Id(point_id));
        }

        Ok(results)
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

    pub fn add_point(&self, point_id: PointId, value: &Value) -> Result<(), sled::Error> {
        let PointId::Id(point_id) = point_id else {
            return Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid PointId type",
            )));
        };

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

    pub fn get_index_names(&self) -> Vec<String> {
        self.indices.keys().cloned().collect()
    }

    pub fn load(db: &Db) -> Self {
        // ToDo: Load indices from the database
        Self::default(db)
    }

    pub fn upsert(&self, point: &Point) -> Result<(), sled::Error> {
        for (index_key, index_tree) in &self.indices {
            if let Some(payload_value) = point.payload.get(index_key) {
                index_tree.add_point(point.id.clone(), payload_value)?;
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
        } else {
            panic!("Expected NumericIndex");
        }
    }
}
