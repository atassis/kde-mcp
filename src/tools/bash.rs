use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BashArgs {
    pub command: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct BashResponse {}

pub async fn bash(_args: BashArgs) -> Result<BashResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
