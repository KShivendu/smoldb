use serde_json::Value;
use std::{cmp::Ordering, collections::HashMap, sync::RwLock};

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

pub type DimType = f64;

pub struct VectorIndex {
    tree: sled::Tree,
    in_memory: Option<RwLock<InMemoryVectorIndex>>,
}

impl FieldIndexTrait<&[DimType]> for VectorIndex {
    fn open(db: &sled::Db, name: &str, use_in_memory: bool) -> StorageResult<Self> {
        // Todo: Take dimension as an argument via payload index config
        let tree = db.open_tree(format!("{name}_vector_index"))?;
        let in_memory = if use_in_memory {
            Some(RwLock::new(InMemoryVectorIndex::new(&tree)?))
        } else {
            None
        };
        Ok(VectorIndex { tree, in_memory })
    }

    fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()> {
        let value = value.as_array().ok_or_else(|| {
            StorageError::BadInput(format!("Vector index value must be an array: {value}"))
        })?;
        let vector = value
            .iter()
            .map(|v| {
                v.as_f64().ok_or_else(|| {
                    StorageError::BadInput(format!(
                        "Vector index value must be an array of numbers: {value:?}"
                    ))
                })
            })
            .collect::<StorageResult<Vec<DimType>>>()?;

        let normalization_factor = vector.iter().map(|v| v * v).sum::<DimType>().sqrt();
        let normalized_vector = vector
            .iter()
            .map(|v| v / normalization_factor)
            .collect::<Vec<DimType>>();
        let encoded_point_ids = encoded_point_ids(&[point_id])?;
        let encoded_vector = encode_vector(&normalized_vector)?;

        self.tree.insert(encoded_point_ids, encoded_vector)?;

        if let Some(in_memory) = &self.in_memory {
            let mut guard = in_memory.write().map_err(|e| {
                StorageError::ServiceError(format!(
                    "Failed to write to in-memory vector index: {e}"
                ))
            })?;
            guard.insert(point_id, vector)?;
        }

        Ok(())
    }

    fn add_points(&self, point_ids: &[u64], values: &[Value]) -> StorageResult<()> {
        for (point_id, value) in point_ids.iter().zip(values.iter()) {
            self.add_point(*point_id, value)?;
        }
        Ok(())
    }

    fn query(
        &self,
        query: &[DimType],
        _operation: &FilterOperator, // todo: allow specifying distance metric?
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        let mut all_vectors = Vec::new();
        if let Some(in_memory) = &self.in_memory {
            let guard = in_memory.read().map_err(|e| {
                StorageError::ServiceError(format!(
                    "Failed to read from in-memory vector index: {e}"
                ))
            })?;
            for (point_id, vector) in guard.iter() {
                all_vectors.push((*point_id, vector.clone())); // todo: avoid cloning
            }
        } else {
            // Read from disk since we don't have in-memory cache
            for result in self.tree.iter() {
                let (key, value) = result?;
                let point_id = decoded_point_ids(&key)?[0];
                let vector = decode_vector(&value)?;
                all_vectors.push((point_id, vector));
            }
        }

        // Only checking if the first vector has the same length as the query
        // But ideally it should be rejected to keep query times fast and just check for corruption on disK?
        if let Some(first_vector) = all_vectors.first() {
            if first_vector.1.len() != query.len() {
                return Err(StorageError::BadInput(format!(
                    "Vectors must have the same length: {:?} and {query:?}",
                    first_vector.1
                )));
            }
        }

        let query_normalization_factor = query.iter().map(|v| v * v).sum::<DimType>().sqrt();
        let normalized_query = query
            .iter()
            .map(|v| v / query_normalization_factor)
            .collect::<Vec<DimType>>();

        let mut results = Vec::new();

        for (point_id, vector) in all_vectors {
            let similarity = cosine_similarity(&vector, &normalized_query);
            results.push((point_id, similarity));
        }

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

        if let Some(limit) = limit {
            results.truncate(limit);
        }

        // Todo: return scores as well
        Ok(results.into_iter().map(|(id, _)| PointId::Id(id)).collect())
    }
}

struct InMemoryVectorIndex {
    index: HashMap<u64, Vec<DimType>>,
}

impl InMemoryVectorIndex {
    pub fn new(tree: &sled::Tree) -> StorageResult<Self> {
        let mut index = HashMap::new();
        for item in tree.iter() {
            let (key, value) = item?;
            let point_id = decoded_point_ids(&key)?[0];
            let vector = decode_vector(&value)?;
            index.insert(point_id, vector);
        }
        Ok(Self { index })
    }

    pub fn insert(&mut self, point_id: u64, vector: Vec<DimType>) -> StorageResult<()> {
        self.index.insert(point_id, vector);
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Vec<DimType>)> {
        self.index.iter()
    }
}

/// Computes the cosine similarity between two vectors
/// Assume the vectors are already normalized
pub fn cosine_similarity(a: &[DimType], b: &[DimType]) -> DimType {
    a.iter().zip(b.iter()).map(|(a, b)| a * b).sum::<DimType>()
}

fn encode_vector(vector: &[DimType]) -> StorageResult<Vec<u8>> {
    bincode::encode_to_vec(vector, bincode::config::standard())
        .map_err(|e| StorageError::CodecError(format!("Failed to encode vector: {e}")))
}

fn decode_vector(data: &[u8]) -> StorageResult<Vec<DimType>> {
    bincode::decode_from_slice(data, bincode::config::standard())
        .map(|(vector, _)| vector)
        .map_err(|e| StorageError::CodecError(format!("Failed to decode vector: {e}")))
}
