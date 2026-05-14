use crate::atspi::ref_encoding::decode_element_ref;
use crate::error::ToolError;
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus::names::BusName;
use atspi::zbus::zvariant::ObjectPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementResponse {
    pub ok: bool,
    pub action_invoked: String,
}

pub async fn act_on_element(
    state: &KdeMcp,
    args: ActOnElementArgs,
) -> Result<ActOnElementResponse, ToolError> {
    let meta = state
        .refs
        .resolve(&args.element_ref)
        .await
        .map_err(|e| match e {
            RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                ref_id: args.element_ref.clone(),
            },
        })?;

    // Policy gate — target derived from the ref's owning window per spec
    // §6.0 step 3. We resolve via the element's AT-SPI bus name's pid →
    // KWin window with matching pid → resourceClass. Falls back to the
    // focused-window class if the lookup fails.
    let target_class = match owning_class_for_element_ref(state, &meta.payload).await {
        Ok(Some(c)) => Some(c),
        _ => crate::policy_target::focused_class(state).await?,
    };
    let target = crate::policy_target::to_target(target_class);
    let policy_warnings = state
        .policy
        .check(target, crate::policy::Action::Generic)
        .await?;
    if meta.kind != RefKind::Element {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("ref {} is not an element", args.element_ref),
        });
    }
    let (bus, path) = decode_element_ref(&meta.payload).ok_or_else(|| ToolError::StaleHandle {
        ref_id: args.element_ref.clone(),
    })?;

    let conn = state.atspi.get().await?;
    let bus_name = BusName::try_from(bus.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad bus name {bus}: {e}"),
    })?;
    let obj_path = ObjectPath::try_from(path.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad path {path}: {e}"),
    })?;

    let acc = AccessibleProxy::builder(conn.connection())
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
        })?;

    let proxies = acc
        .proxies()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;
    let action_proxy = proxies
        .action()
        .await
        .map_err(|_| ToolError::NotActionable {
            available_actions: vec![],
            fallback_suggestion: Some(
                "element has no Action interface; try mouse.click on bbox".into(),
            ),
        })?;
    let actions = action_proxy
        .get_actions()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    let requested = args.action.to_ascii_lowercase();
    let idx = actions
        .iter()
        .position(|a| a.name.eq_ignore_ascii_case(&requested))
        .ok_or_else(|| ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some(format!(
                "no action named {requested:?}; try one of available_actions"
            )),
        })? as i32;

    let invoked = action_proxy
        .do_action(idx)
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;
    if !invoked {
        return Err(ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some("AT-SPI do_action returned false".into()),
        });
    }
    let _ = policy_warnings; // attached via response if/when schema is widened
    Ok(ActOnElementResponse {
        ok: true,
        action_invoked: actions[idx as usize].name.clone(),
    })
}

/// Map an element ref's encoded bus name → pid via `GetConnectionUnixProcessID`
/// → matching KWin window's resourceClass. Returns None if any step fails
/// (caller falls back to focused-window heuristic).
async fn owning_class_for_element_ref(
    state: &KdeMcp,
    payload: &str,
) -> Result<Option<String>, ToolError> {
    let (bus, _path) = match decode_element_ref(payload) {
        Some(p) => p,
        None => return Ok(None),
    };
    let conn = state.atspi.get().await?;
    let dbus = zbus::fdo::DBusProxy::new(conn.connection())
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;
    let bus_name = match zbus::names::BusName::try_from(bus.to_string()) {
        Ok(n) => n,
        Err(_) => return Ok(None),
    };
    let pid = match dbus.get_connection_unix_process_id(bus_name).await {
        Ok(p) => p as i64,
        Err(_) => return Ok(None),
    };
    // KWin script: find the window with this pid, return its class.
    let body = format!(
        r#"
        var ws = workspace;
        var list = ws.windowList();
        for (var i = 0; i < list.length; i++) {{
            if (list[i].pid === {pid}) {{
                return {{ class: list[i].resourceClass || "" }};
            }}
        }}
        return {{ class: "" }};
        "#
    );
    let resp =
        crate::kwin::scripting::run_script(&state.bus, &body, Duration::from_secs(2)).await?;
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap_or(serde_json::Value::Null);
    Ok(v.get("class")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
}
