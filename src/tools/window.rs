use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryArgs {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct WindowQueryResponse {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowControlArgs {
    pub window_ref: String,
    pub action: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct WindowControlResponse {}

pub async fn window_query(_args: WindowQueryArgs) -> Result<WindowQueryResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn window_control(_args: WindowControlArgs) -> Result<WindowControlResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
