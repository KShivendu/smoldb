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
    // Represent as String in API:
    #[schema(value_type = String, example = "550e8400-e29b-41d4-a716-446655440000")]
    Uuid(uuid::Uuid),
}

impl From<u64> for PointId {
    fn from(id: u64) -> Self {
        PointId::Id(id)
    }
}

impl TryFrom<&str> for PointId {
    type Error = StorageError;

    fn try_from(uuid: &str) -> Result<Self, Self::Error> {
        let uuid = uuid::Uuid::parse_str(uuid)
            .map_err(|e| StorageError::CodecError(format!("Invalid UUID: {e}")))?;
        Ok(PointId::Uuid(uuid))
    }
}

impl PointId {
    pub fn encode(&self) -> Result<Vec<u8>, StorageError> {
        let res = match self {
            PointId::Id(id) => {
                let mut key = Vec::with_capacity(9);
                key.push(0x00); // discriminant for numeric ID
                key.extend_from_slice(&id.to_be_bytes());
                key
            }
            PointId::Uuid(uuid) => {
                // ToDo: Check if bincode is more performant for uuid than uuid.as_bytes()?
                // u64 is clearly more performant with .to_be_bytes() than bincode
                // Ref: https://github.com/KShivendu/smoldb/pull/82#issuecomment-3650742246
                let mut key = Vec::with_capacity(17);
                key.push(0x01); // discriminant for UUID
                key.extend_from_slice(uuid.as_bytes()); // 16 bytes in big-endian
                key
            }
        };
        Ok(res)
    }

    pub fn decode(data: &[u8]) -> Result<PointId, StorageError> {
        match data.first() {
            Some(&0x00) if data.len() == 9 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&data[1..9]);
                Ok(PointId::Id(u64::from_be_bytes(buf)))
            }
            Some(&0x01) if data.len() == 17 => {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&data[1..17]);
                Ok(PointId::Uuid(uuid::Uuid::from_bytes(buf)))
            }
            _ => Err(StorageError::CodecError(format!(
                "Invalid PointId key: {data:?}"
            ))),
        }
    }
}

impl Point {
    pub fn encode_payload(&self) -> Result<Vec<u8>, StorageError> {
        serde_cbor::to_vec(&self.payload)
            .map_err(|e| StorageError::CodecError(format!("Failed to serialize point: {e}")))
    }

    pub fn decode_payload(data: &[u8]) -> Result<serde_json::Value, StorageError> {
        serde_cbor::from_slice(data)
            .map_err(|e| StorageError::CodecError(format!("Failed to deserialize point: {e}")))
    }

    pub fn decode(key: &[u8], value: &[u8]) -> Result<Point, StorageError> {
        Ok(Point {
            id: PointId::decode(key)?,
            payload: Point::decode_payload(value)?,
        })
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
        let encoded = point.encode_payload().unwrap();
        assert_eq!(encoded, vec![161, 101, 112, 114, 105, 99, 101, 24, 100]);
        let decoded = Point::decode(&point.id.encode().unwrap(), &encoded).unwrap();
        assert_eq!(point, decoded);
    }
}
