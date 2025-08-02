use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Gte,
    Eq,
    Lte,
    Gt,
    Lt,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct QueryFilter {
    pub key: String,
    pub value: String,
    pub op: FilterOperator,
}
