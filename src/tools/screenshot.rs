use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotArgs {
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub include_cursor: Option<bool>,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ScreenshotResponse {}

pub async fn screenshot(
    _state: &KdeMcp,
    _args: ScreenshotArgs,
) -> Result<ScreenshotResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
