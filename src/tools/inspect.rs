use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InspectArgs {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub max_elements: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct InspectResponse {}

pub async fn inspect(_state: &KdeMcp, _args: InspectArgs) -> Result<InspectResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
