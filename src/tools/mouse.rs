use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MouseArgs {
    pub action: String,
    #[serde(default)]
    pub coordinate: Option<[i32; 2]>,
    #[serde(default)]
    pub button: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MouseResponse {}

pub async fn mouse(_state: &KdeMcp, _args: MouseArgs) -> Result<MouseResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
