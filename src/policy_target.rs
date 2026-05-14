//! Target-app resolution for the policy gate — spec §6.0 step 3.
//!
//! Each destructive tool needs to know which app class is about to be
//! affected, so the gate can look up the tier / sentinel / approval state.
//! The rules per tool:
//!
//! - `act_on_element` / `window_control` → resolved from the ref's owning
//!   window. The ref store holds the encoded payload; for window refs the
//!   payload is the KWin internal UUID, and we ask KWin for the class.
//! - `mouse` → KWin script: "window at coordinate". For `mouse.scroll`
//!   without coord, falls back to the active window.
//! - `keyboard.{type,key}` → the currently-focused window's class.
//! - `bash` / `text_editor.edit` / `clipboard_write` → Classless (see
//!   [`crate::policy::TargetApp::Classless`]).

use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::policy::TargetApp;
use crate::server::KdeMcp;
use std::time::Duration;

/// Returns the resource class of the currently-focused window, or `None`
/// if no window is focused.
pub async fn focused_class(state: &KdeMcp) -> Result<Option<String>, ToolError> {
    let payload = run_script(&state.bus, JS_ACTIVE_CLASS, Duration::from_secs(2)).await?;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("focused_class JSON parse: {e}"),
        })?;
    Ok(v.get("class")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
}

/// Returns the resource class of the window at logical-pixel coordinate
/// `(x, y)`. Falls back to `None` if no window is at that point.
pub async fn class_at_coord(state: &KdeMcp, x: i32, y: i32) -> Result<Option<String>, ToolError> {
    let body = format!(
        r#"
        var ws = workspace;
        var w = ws.windowAt ? ws.windowAt(Qt.point({x}, {y}), 1) : null;
        var pick = (w && w.length > 0) ? w[0] : null;
        return {{ class: pick ? pick.resourceClass : "" }};
        "#
    );
    let payload = run_script(&state.bus, &body, Duration::from_secs(2)).await?;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("class_at_coord JSON parse: {e}"),
        })?;
    Ok(v.get("class")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
}

/// Returns the class of the window owning the given window-UUID. If
/// `window_uuid` is a stored opaque ref, resolve it first.
pub async fn class_for_window_uuid(
    state: &KdeMcp,
    raw_uuid: &str,
) -> Result<Option<String>, ToolError> {
    let body = format!(
        r#"
        var ws = workspace;
        var found = null;
        var list = ws.windowList();
        for (var i = 0; i < list.length; i++) {{
            if (list[i].internalId && list[i].internalId.toString() === "{raw_uuid}") {{
                found = list[i];
                break;
            }}
        }}
        return {{ class: found ? found.resourceClass : "" }};
        "#
    );
    let payload = run_script(&state.bus, &body, Duration::from_secs(2)).await?;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("class_for_window_uuid JSON parse: {e}"),
        })?;
    Ok(v.get("class")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
}

/// Convenience wrapper: `Option<String> -> TargetApp`. None → Classless.
/// Used when a tool can't pin down the target app (e.g. mouse.scroll
/// without coord on a desktop with no focused window) — we degrade to
/// Classless rather than blocking. The audit log captures the tool call
/// regardless.
pub fn to_target(class: Option<String>) -> TargetApp {
    match class {
        Some(c) => TargetApp::Class(c),
        None => TargetApp::Classless,
    }
}

const JS_ACTIVE_CLASS: &str = r#"
    var ws = workspace;
    var w = ws.activeWindow;
    return { class: w ? w.resourceClass : "" };
"#;
