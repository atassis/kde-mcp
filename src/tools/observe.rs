use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ObserveArgs {
    #[serde(default)]
    pub detail: Option<DetailLevel>,
    #[serde(default)]
    pub include_other_windows: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Summary,
    #[default]
    Interactive,
    Full,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ObserveResponse {}

pub async fn observe(_args: ObserveArgs) -> Result<ObserveResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
