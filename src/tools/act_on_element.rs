use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ActOnElementResponse {}

pub async fn act_on_element(
    _state: &KdeMcp,
    _args: ActOnElementArgs,
) -> Result<ActOnElementResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
