use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReadTextArgs {
    pub region: String,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub include_bboxes: Option<bool>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ReadTextResponse {}

pub async fn read_text(_args: ReadTextArgs) -> Result<ReadTextResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
