use crate::atspi::ref_encoding::decode_element_ref;
use crate::atspi::tree::{FlatElement, WalkOpts, walk};
use crate::error::ToolError;
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use atspi::AccessibilityConnection;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::zbus::names::BusName;
use atspi::zbus::zvariant::ObjectPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InspectArgs {
    /// `"focused"` | `"window:<ref>"` | `"element:<ref>"`. Default: `"focused"`.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub role_filter: Option<Vec<String>>,
    #[serde(default)]
    pub name_pattern: Option<String>,
    #[serde(default)]
    pub max_elements: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InspectResponse {
    pub elements: Vec<FlatElement>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn inspect(state: &KdeMcp, args: InspectArgs) -> Result<InspectResponse, ToolError> {
    let conn = state.atspi.get().await?;
    let target = args.target.unwrap_or_else(|| "focused".into());

    let root = match target.as_str() {
        "focused" => {
            // Phase 1 first cut: focus tracking requires a long-running
            // signal subscription on the a11y bus. The spec defers that to
            // Phase 4. Until then, "focused" returns a structured error
            // pointing at "element:<ref>" / "window:<ref>" as the workaround.
            return Err(ToolError::NotImplemented);
        }
        s if s.starts_with("window:") => {
            // Window→AT-SPI app mapping is non-trivial (window pid → bus
            // name discovery via Application introspection). Phase 4 absorbs.
            return Err(ToolError::NotImplemented);
        }
        s if s.starts_with("element:") => {
            let ref_id = &s["element:".len()..];
            let meta = state.refs.resolve(ref_id).await.map_err(|e| match e {
                RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                    ref_id: ref_id.into(),
                },
            })?;
            if meta.kind != RefKind::Element {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("ref {ref_id} is not an element"),
                });
            }
            atspi_lookup_by_encoded(conn, &meta.payload).await?
        }
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unrecognised target: {other}"),
            });
        }
    };

    let name_pattern = args
        .name_pattern
        .as_deref()
        .map(regex_lite::Regex::new)
        .transpose()
        .map_err(|e| ToolError::UnsupportedRegion {
            reason: format!("name_pattern: {e}"),
        })?;

    let opts = WalkOpts {
        max_depth: args.max_depth.unwrap_or(3),
        max_elements: args.max_elements.unwrap_or(200) as usize,
        role_filter: args.role_filter,
        name_pattern,
    };

    let (elements, truncated, warnings) = walk(conn, root, state.refs.clone(), opts).await?;
    Ok(InspectResponse {
        elements,
        truncated,
        warnings,
    })
}

/// Resolve an encoded `(bus_name|object_path)` payload back into an
/// AccessibleProxy. The encoded form comes from a previously-minted element
/// ref (see `crate::atspi::ref_encoding`).
async fn atspi_lookup_by_encoded<'a>(
    conn: &'a AccessibilityConnection,
    payload: &str,
) -> Result<AccessibleProxy<'a>, ToolError> {
    let (bus, path) = decode_element_ref(payload).ok_or_else(|| ToolError::StaleHandle {
        ref_id: payload.to_string(),
    })?;
    let bus_name = BusName::try_from(bus.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("invalid bus name {bus}: {e}"),
    })?;
    let obj_path = ObjectPath::try_from(path.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("invalid object path {path}: {e}"),
    })?;
    AccessibleProxy::builder(conn.connection())
        .destination(bus_name)
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?
        .path(obj_path)
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?
        .cache_properties(atspi::zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })
}
