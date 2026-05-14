use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WaitArgs {
    pub condition: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub seconds: Option<f64>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct WaitResponse {}

pub async fn wait(_args: WaitArgs) -> Result<WaitResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
