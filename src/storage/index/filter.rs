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

impl FilterOperator {
    pub fn as_str(&self) -> &str {
        match self {
            FilterOperator::Gte => "gte",
            FilterOperator::Eq => "eq",
            FilterOperator::Lte => "lte",
            FilterOperator::Gt => "gt",
            FilterOperator::Lt => "lt",
        }
    }
}

impl From<String> for FilterOperator {
    fn from(op: String) -> Self {
        match op.as_str() {
            "gte" => FilterOperator::Gte,
            "eq" => FilterOperator::Eq,
            "lte" => FilterOperator::Lte,
            "gt" => FilterOperator::Gt,
            "lt" => FilterOperator::Lt,
            _ => panic!("Unknown filter operator: {op}"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct QueryFilter {
    pub key: String,
    pub value: String,
    pub op: FilterOperator,
}

impl QueryFilter {
    pub fn new(key: impl Into<String>, value: impl Into<String>, op: FilterOperator) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            op,
        }
    }
}
