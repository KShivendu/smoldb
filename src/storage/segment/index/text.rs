use serde_json::Value;
use sled::Db;

use crate::storage::{
    index::{
        filter::FilterOperator,
        payload_index::{decoded_point_ids, encoded_point_ids, FieldIndexTrait},
    },
    segment::PointId,
};

pub struct TextIndex(sled::Tree);

// Full text search index implementation with posting lists
impl TextIndex {
    pub fn upsert(&self, point_id: u64, text: &str) -> Result<(), sled::Error> {
        // TODO: Add stop words, stemming, etc based on language
        // We need to tokenize the text into terms
        let terms: Vec<&str> = text.split_whitespace().collect();

        // In this tree, the term becomes the key, and the point/doc ID is part of a list of values.

        for term in terms {
            let term_key = term.as_bytes();

            let mut point_ids: Vec<u64> = self
                .0
                .get(term_key)?
                .map(|data| {
                    // Decode existing point IDs for this term
                    // Todo: Apply delta encoding??
                    decoded_point_ids(&data).unwrap_or_else(|e| {
                        panic!("Failed to decode point IDs from posting list: {e}")
                    })
                })
                .unwrap_or_default();

            // Add point and sort
            point_ids.push(point_id);
            point_ids.sort();

            // Store back
            let encoded_ids = encoded_point_ids(&point_ids).map_err(|e| {
                sled::Error::Io(std::io::Error::other(format!(
                    "Failed to encode point IDs for posting list: {e}"
                )))
            })?;

            self.0.insert(term_key, encoded_ids).map_err(|e| {
                sled::Error::Io(std::io::Error::other(format!(
                    "Failed to insert into text index tree: {e}"
                )))
            })?;
        }

        Ok(())
    }
}

impl FieldIndexTrait<&str> for TextIndex {
    fn open(db: &Db, name: &str) -> Self {
        let tree = db
            .open_tree(format!("{name}_text_index"))
            .expect("Failed to open sled tree");
        Self(tree)
    }

    fn add_point(&self, point_id: u64, value: &Value) -> Result<(), sled::Error> {
        match value {
            Value::String(text) => self.upsert(point_id, text.as_str()),
            _ => Err(sled::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{value} is not a valid string value"),
            ))),
        }
    }

    fn query(
        &self,
        value: &str,
        _operation: &FilterOperator, // todo: Remove operation for text index?
        limit: Option<usize>,
    ) -> Result<Vec<PointId>, sled::Error> {
        let mut results = Vec::new();
        let query_key = value.as_bytes();

        let point_ids: Vec<u64> = self
            .0
            .get(query_key)?
            .map(|data| {
                // Decode existing point IDs for this term
                decoded_point_ids(&data)
                    .unwrap_or_else(|e| panic!("Failed to decode point IDs from posting list: {e}"))
            })
            .unwrap_or_default();

        for point_id in point_ids {
            results.push(PointId::Id(point_id));
            if let Some(lim) = limit {
                if results.len() >= lim {
                    break;
                }
            }
        }

        Ok(results)
    }
}

// TextIndex is basically going to be {term -> list of point ids}
//
