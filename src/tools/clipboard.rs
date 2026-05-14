use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ClipboardReadArgs {}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardReadResponse {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipboardWriteArgs {
    pub content: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardWriteResponse {}

pub async fn clipboard_read(
    _args: ClipboardReadArgs,
) -> Result<ClipboardReadResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn clipboard_write(
    _args: ClipboardWriteArgs,
) -> Result<ClipboardWriteResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
