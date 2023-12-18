use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Presence {
    pub metas: Vec<Meta>,
}
impl Presence {
    pub(crate) fn join_references(&self) -> Vec<String> {
        self.metas
            .iter()
            .map(|meta| meta.join_reference.clone())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Meta {
    #[serde(rename(deserialize = "phx_ref"))]
    pub join_reference: String,
    #[serde(flatten)]
    pub others: HashMap<String, Value>,
}
