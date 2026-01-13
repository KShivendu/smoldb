use serde_json::Value;

use crate::{
    error::StorageResult,
    storage::{
        index::{filter::FilterOperator, payload_index::FieldIndexTrait},
        segment::PointId,
    },
};

pub type VectorDataType = f64;

pub struct VectorIndex {}

impl FieldIndexTrait<&[VectorDataType]> for VectorIndex {
    fn open(_db: &sled::Db, _name: &str, _use_in_memory: bool) -> StorageResult<Self> {
        Ok(VectorIndex {})
    }

    fn add_point(&self, _point_id: u64, _value: &Value) -> StorageResult<()> {
        Ok(())
    }

    fn add_points(&self, _point_ids: &[u64], _values: &[Value]) -> StorageResult<()> {
        Ok(())
    }

    fn query(
        &self,
        _value: &[VectorDataType],
        _operation: &FilterOperator,
        _limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        Ok(Vec::new())
    }
}
