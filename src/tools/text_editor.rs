use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorViewArgs {
    pub path: String,
    #[serde(default)]
    pub view_range: Option<[i32; 2]>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TextEditorViewResponse {}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorEditArgs {
    pub command: String,
    pub path: String,
    #[serde(default)]
    pub old_str: Option<String>,
    #[serde(default)]
    pub new_str: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TextEditorEditResponse {}

pub async fn text_editor_view(
    _state: &KdeMcp,
    _args: TextEditorViewArgs,
) -> Result<TextEditorViewResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn text_editor_edit(
    _state: &KdeMcp,
    _args: TextEditorEditArgs,
) -> Result<TextEditorEditResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
