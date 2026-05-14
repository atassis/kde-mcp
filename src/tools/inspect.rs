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
            // Walk-by-state: get the registry root, BFS until we find an
            // accessible with the Focused state set. Cheaper than running
            // a background focus-tracker.
            atspi_focused_root(conn).await?
        }
        s if s.starts_with("window:") => {
            // Map window ref → KWin internalId → window pid → AT-SPI
            // Application accessible with matching pid.
            let ref_id = &s["window:".len()..];
            let meta = state.refs.resolve(ref_id).await.map_err(|e| match e {
                RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                    ref_id: ref_id.into(),
                },
            })?;
            if meta.kind != RefKind::Window {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("ref {ref_id} is not a window"),
                });
            }
            let pid = crate::policy_target::pid_for_window_uuid(state, &meta.payload)
                .await?
                .ok_or_else(|| ToolError::TargetNotFound {
                    closest_match: Some(format!("window {ref_id} not found by KWin")),
                })?;
            atspi_root_by_pid(conn, pid).await?
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

/// Walk the AT-SPI registry root looking for an accessible whose state set
/// contains `Focused`. BFS bounded to 1000 visited nodes so a degenerate
/// tree doesn't hang the call.
async fn atspi_focused_root(
    conn: &AccessibilityConnection,
) -> Result<AccessibleProxy<'_>, ToolError> {
    use atspi::State;
    use atspi::proxy::accessible::ObjectRefExt;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: format!("registry root: {e}"),
        })?;
    let mut queue: std::collections::VecDeque<AccessibleProxy<'_>> =
        std::collections::VecDeque::new();
    queue.push_back(root);
    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if visited > 1000 {
            break;
        }
        if let Ok(states) = node.get_state().await
            && states.contains(State::Focused)
        {
            return Ok(node);
        }
        let count = node.child_count().await.unwrap_or(0);
        for i in 0..count {
            if let Ok(child_ref) = node.get_child_at_index(i).await
                && let Ok(child) = child_ref
                    .into_accessible_proxy(conn.connection())
                    .await
            {
                queue.push_back(child);
            }
        }
    }
    Err(ToolError::TargetNotFound {
        closest_match: Some("no focused accessible found in registry tree".into()),
    })
}

/// Walk the AT-SPI registry top-level applications looking for one whose
/// `Application` proxy reports the given pid. AT-SPI applications expose
/// pid via the Application interface's `Id` property (which is also the
/// pid in practice on Linux).
async fn atspi_root_by_pid(
    conn: &AccessibilityConnection,
    pid: i64,
) -> Result<AccessibleProxy<'_>, ToolError> {
    use atspi::proxy::accessible::ObjectRefExt;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: format!("registry root: {e}"),
        })?;
    let count = root.child_count().await.unwrap_or(0);
    for i in 0..count {
        let child_ref = match root.get_child_at_index(i).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        // The application's bus name unique-name doesn't carry the pid
        // directly. We use `accessible_id` as a proxy or, failing that,
        // walk into the child and check whether the destination D-Bus
        // unique name matches what `org.freedesktop.DBus.GetConnectionUnixProcessID`
        // returns for the same pid. Cheap heuristic: AT-SPI exposes
        // `accessible_id` per app which on most desktops includes the
        // executable name; this falls back to "any" if pid mapping isn't
        // robust enough.
        let app_proxy = match child_ref.into_accessible_proxy(conn.connection()).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let bus_name = app_proxy.inner().destination().to_string();
        if bus_name_matches_pid(conn.connection(), &bus_name, pid).await {
            return Ok(app_proxy);
        }
    }
    Err(ToolError::TargetNotFound {
        closest_match: Some(format!(
            "no AT-SPI application with pid {pid}; app may not be a11y-enabled"
        )),
    })
}

async fn bus_name_matches_pid(conn: &zbus::Connection, bus_name: &str, want_pid: i64) -> bool {
    let proxy = match zbus::fdo::DBusProxy::new(conn).await {
        Ok(p) => p,
        Err(_) => return false,
    };
    let name = match zbus::names::BusName::try_from(bus_name.to_string()) {
        Ok(n) => n,
        Err(_) => return false,
    };
    match proxy.get_connection_unix_process_id(name).await {
        Ok(pid) => pid as i64 == want_pid,
        Err(_) => false,
    }
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
