use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct ClipboardReadArgs {}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardReadResponse {}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
pub struct ClipboardWriteArgs {
    pub content: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardWriteResponse {}

pub async fn clipboard_read(
    _state: &KdeMcp,
    _args: ClipboardReadArgs,
) -> Result<ClipboardReadResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn clipboard_write(
    _state: &KdeMcp,
    _args: ClipboardWriteArgs,
) -> Result<ClipboardWriteResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
