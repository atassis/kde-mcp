use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ActOnElementResponse {}

pub async fn act_on_element(_args: ActOnElementArgs) -> Result<ActOnElementResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
