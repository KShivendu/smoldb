use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::StorageError;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ToSchema)]
pub struct Point {
    pub id: PointId,
    pub payload: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug, ToSchema)]
#[serde(untagged)]
pub enum PointId {
    Id(u64),
    Uuid(String),
}

impl From<u64> for PointId {
    fn from(id: u64) -> Self {
        PointId::Id(id)
    }
}

impl From<String> for PointId {
    fn from(uuid: String) -> Self {
        PointId::Uuid(uuid)
    }
}

impl PointId {
    pub fn into_string(&self) -> String {
        match self {
            PointId::Id(id) => id.to_string(),
            PointId::Uuid(uuid) => uuid.clone(),
        }
    }
}

impl Point {
    pub fn encode(&self) -> Result<Vec<u8>, StorageError> {
        serde_cbor::to_vec(self)
            .map_err(|e| StorageError::ServiceError(format!("Failed to serialize point: {e}")))
    }

    pub fn decode(data: &[u8]) -> Result<Self, StorageError> {
        serde_cbor::from_slice(data)
            .map_err(|e| StorageError::ServiceError(format!("Failed to deserialize point: {e}")))
    }
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_point_encode_decode() {
        let point = Point {
            id: PointId::Id(1),
            payload: json!({ "price": 100 }),
        };
        let encoded = point.encode().unwrap();
        assert_eq!(
            encoded,
            vec![
                162, 98, 105, 100, 1, 103, 112, 97, 121, 108, 111, 97, 100, 161, 101, 112, 114,
                105, 99, 101, 24, 100
            ]
        );
        let decoded = Point::decode(&encoded).unwrap();
        assert_eq!(point, decoded);
    }
}
