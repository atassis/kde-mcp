use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Split per spec §4.5 poka-yoke: type accepts text only; key accepts keys
// only. "Type while holding Ctrl" can only be expressed by two sequential
// calls, which is what we want.

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardTypeArgs {
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardKeyArgs {
    pub keys: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct KeyboardResponse {}

pub async fn keyboard_type(
    _state: &KdeMcp,
    _args: KeyboardTypeArgs,
) -> Result<KeyboardResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn keyboard_key(
    _state: &KdeMcp,
    _args: KeyboardKeyArgs,
) -> Result<KeyboardResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
