use std::{collections::HashMap, sync::RwLock};

use serde_json::Value;
use sled::Db;

use crate::{
    error::{StorageError, StorageResult},
    storage::{
        index::{
            filter::FilterOperator,
            payload_index::{decoded_point_ids, encoded_point_ids, FieldIndexTrait},
        },
        segment::PointId,
    },
};

pub struct TextIndex {
    db: sled::Tree,
    in_memory_index: InMemoryTextIndex,
    use_in_memory: bool,
}

// Full text search index implementation with posting lists
impl TextIndex {
    pub fn upsert(&self, point_id: u64, text: &str) -> StorageResult<()> {
        // TODO: Add stop words, stemming, etc based on language
        // We need to tokenize the text into terms
        let terms: Vec<&str> = text.split_whitespace().collect();

        // In this tree, the term becomes the key, and the point/doc ID is part of a list of values.

        for term in terms {
            let term_key = term.as_bytes();

            let mut point_ids: Vec<u64> = self
                .db
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
            let encoded_ids = encoded_point_ids(&point_ids)?;

            self.db.insert(term_key, encoded_ids).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert into text index tree: {e}"))
            })?;

            if self.use_in_memory {
                self.in_memory_index.override_posting_list(term, point_ids);
            }
        }

        Ok(())
    }
}

impl FieldIndexTrait<&str> for TextIndex {
    fn open(db: &Db, name: &str) -> StorageResult<Self> {
        let tree = db.open_tree(format!("{name}_text_index"))?;

        let in_memory_index = InMemoryTextIndex::new(db);

        Ok(Self {
            db: tree,
            in_memory_index,
            use_in_memory: false,
        })
    }

    fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()> {
        match value {
            Value::String(text) => self.upsert(point_id, text.as_str()),
            _ => Err(StorageError::BadInput(format!(
                "{value} is not a valid string value"
            ))),
        }
    }

    fn query(
        &self,
        value: &str,
        _operation: &FilterOperator, // todo: Remove operation for text index?
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        if self.use_in_memory {
            return Ok(self
                .in_memory_index
                .query(value, limit)
                .into_iter()
                .map(PointId::Id)
                .collect());
        }

        let mut results = Vec::new();
        let query_key = value.as_bytes();

        // Decode existing point IDs for this term
        let point_ids = decoded_point_ids(&self.db.get(query_key)?.unwrap_or_default())?;

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

struct InMemoryTextIndex {
    // Todo: Use ahashmap here
    index: RwLock<HashMap<String, Vec<u64>>>,
}

impl InMemoryTextIndex {
    pub fn new(db: &Db) -> Self {
        let mut index = HashMap::new();

        for item in db.iter() {
            let (key, value) = item.expect("Failed to read text index item");
            let term = String::from_utf8(key.to_vec()).expect("Invalid UTF-8 in key");
            let point_ids = decoded_point_ids(&value).expect("Failed to decode point IDs");
            index.insert(term, point_ids);
        }

        Self {
            index: RwLock::new(index),
        }
    }

    pub fn override_posting_list(&self, term: &str, point_ids: Vec<u64>) {
        let mut index = self.index.write().unwrap();
        index.insert(term.to_string(), point_ids);
    }

    pub fn query(&self, term: &str, limit: Option<usize>) -> Vec<u64> {
        match self.index.read().unwrap().get(term) {
            Some(results) => match limit {
                Some(limit) => results.iter().take(limit).cloned().collect(),
                None => results.clone(),
            },
            None => Vec::new(),
        }
    }
}
