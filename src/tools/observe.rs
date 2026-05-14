use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ObserveArgs {
    #[serde(default)]
    pub detail: Option<DetailLevel>,
    #[serde(default)]
    pub include_other_windows: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Summary,
    #[default]
    Interactive,
    Full,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ObserveResponse {}

pub async fn observe(_state: &KdeMcp, _args: ObserveArgs) -> Result<ObserveResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
