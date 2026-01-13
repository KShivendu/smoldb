use serde_json::Value;
use std::cmp::Ordering;
use std::cmp::Ordering;

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

pub type VectorDataType = f64;

pub struct VectorIndex {
    tree: sled::Tree,
}

impl FieldIndexTrait<&[VectorDataType]> for VectorIndex {
    fn open(db: &sled::Db, name: &str, _use_in_memory: bool) -> StorageResult<Self> {
        // Todo: Take dimension as an argument via payload index config
        let tree = db.open_tree(format!("{name}_vector_index"))?;
        Ok(VectorIndex { tree })
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
            .collect::<StorageResult<Vec<VectorDataType>>>()?;

        let encoded_point_ids = encoded_point_ids(&[point_id])?;
        let encoded_vector = encode_vector(&vector)?;
        self.tree.insert(encoded_point_ids, encoded_vector)?;
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
        query: &[VectorDataType],
        _operation: &FilterOperator, // todo: allow specifying distance metric?
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        let mut all_vectors = Vec::new();
        for result in self.tree.iter() {
            let (key, value) = result?;
            let point_id = decoded_point_ids(&key)?[0];
            let vector = decode_vector(&value)?;
            all_vectors.push((point_id, vector));
        }

        let mut results = Vec::new();

        for (point_id, vector) in all_vectors {
            let similarity = cosine_similarity(&vector, query)?;
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

fn cosine_similarity(a: &[VectorDataType], b: &[VectorDataType]) -> Result<f64, StorageError> {
    if a.len() != b.len() {
        return Err(StorageError::BadInput(format!(
            "Vectors must have the same length: {a:?} and {b:?}"
        )));
    }

    let dot_product = a.iter().zip(b.iter()).map(|(a, b)| a * b).sum::<f64>();
    let a_norm = a.iter().map(|a| a * a).sum::<f64>().sqrt();
    let b_norm = b.iter().map(|b| b * b).sum::<f64>().sqrt();
    Ok(dot_product / (a_norm * b_norm))
}

fn encode_vector(vector: &[VectorDataType]) -> StorageResult<Vec<u8>> {
    bincode::encode_to_vec(vector, bincode::config::standard())
        .map_err(|e| StorageError::CodecError(format!("Failed to encode vector: {e}")))
}

fn decode_vector(data: &[u8]) -> StorageResult<Vec<VectorDataType>> {
    bincode::decode_from_slice(data, bincode::config::standard())
        .map(|(vector, _)| vector)
        .map_err(|e| StorageError::CodecError(format!("Failed to decode vector: {e}")))
}
