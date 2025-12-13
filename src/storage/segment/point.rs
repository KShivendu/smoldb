use bincode::{config::Configuration, Decode, Encode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::StorageError;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ToSchema)]
pub struct Point {
    pub id: PointId,
    pub payload: serde_json::Value,
}

#[derive(
    Serialize,
    Deserialize,
    Clone,
    Hash,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Debug,
    ToSchema,
    Encode,
    Decode,
)]
#[serde(untagged)]
pub enum PointId {
    Id(u64),
    Uuid(String),
}

impl PointId {
    pub fn into_string(&self) -> String {
        match self {
            PointId::Id(id) => id.to_string(),
            PointId::Uuid(uuid) => uuid.clone(),
        }
    }
}

impl Encode for Point {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        let json_string = self.payload.to_string();
        json_string.encode(encoder)?;
        Ok(())
    }
}

impl Decode<()> for Point {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let id = PointId::decode(decoder)?;
        let json_string = String::decode(decoder)?;
        let payload = serde_json::from_str(&json_string)
            .map_err(|e| bincode::error::DecodeError::OtherString(e.to_string()))?;
        Ok(Self { id, payload })
    }
}

impl Point {
    pub fn encode(&self) -> Result<Vec<u8>, StorageError> {
        bincode::encode_to_vec(self, bincode_configuration())
            .map_err(|e| StorageError::ServiceError(format!("Failed to serialize point: {e}")))
    }

    pub fn decode(data: &[u8]) -> Result<Self, StorageError> {
        bincode::decode_from_slice(data, bincode_configuration())
            .map(|(point, _)| point)
            .map_err(|e| StorageError::ServiceError(format!("Failed to deserialize point: {e}")))
    }
}

pub fn bincode_configuration() -> Configuration {
    bincode::config::standard().with_variable_int_encoding()
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
            vec![0, 1, 13, 123, 34, 112, 114, 105, 99, 101, 34, 58, 49, 48, 48, 125]
        );
        let decoded = Point::decode(&encoded).unwrap();
        assert_eq!(point, decoded);
    }
}
