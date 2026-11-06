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
//! - `clipboard_write` → Classless (see
//!   [`crate::policy::TargetApp::Classless`]).

use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::policy::TargetApp;
use crate::server::KdeMcp;
use std::time::Duration;

/// Capture identity as well as class: approving one Kate window must not
/// silently authorize input to another Kate window after a long user wait.
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct TargetSnapshot {
    pub class: Option<String>,
    uuid: Option<String>,
    geometry: Option<[i32; 4]>,
    cursor: Option<[i32; 2]>,
}

impl TargetSnapshot {
    pub fn ensure_unchanged(&self, current: &Self) -> Result<(), ToolError> {
        if self != current {
            return Err(ToolError::PermissionDenied {
                reason: "input target changed while awaiting approval; observe and retry".into(),
            });
        }
        Ok(())
    }
}

async fn snapshot(state: &KdeMcp, body: &str) -> Result<TargetSnapshot, ToolError> {
    let payload = run_script(&state.bus, body, Duration::from_secs(2)).await?;
    serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
        stderr: format!("target snapshot JSON parse: {e}"),
    })
}

pub(crate) async fn focused_target(state: &KdeMcp) -> Result<TargetSnapshot, ToolError> {
    snapshot(state, &target_script("ws.activeWindow", false)).await
}

pub async fn focused_class(state: &KdeMcp) -> Result<Option<String>, ToolError> {
    Ok(focused_target(state).await?.class)
}

pub(crate) async fn target_at_coord(
    state: &KdeMcp,
    x: i32,
    y: i32,
) -> Result<TargetSnapshot, ToolError> {
    snapshot(state, &class_at_coord_script(x, y)).await
}

pub(crate) async fn pointer_target(state: &KdeMcp) -> Result<TargetSnapshot, ToolError> {
    snapshot(
        state,
        &target_script("(ws.windowAt ? ws.windowAt(ws.cursorPos, 1) : [])[0]", true),
    )
    .await
}

fn target_script(pick: &str, include_cursor: bool) -> String {
    let cursor = if include_cursor {
        "[ws.cursorPos.x, ws.cursorPos.y]"
    } else {
        "null"
    };
    format!(
        r#"
        var ws = workspace;
        var pick = {pick};
        var g = pick ? pick.frameGeometry : null;
        return {{ class: pick ? pick.resourceClass : null,
                  uuid: pick && pick.internalId ? pick.internalId.toString() : null,
                  geometry: g ? [g.x, g.y, g.width, g.height] : null,
                  cursor: {cursor} }};
    "#
    )
}

/// KWin's QJSEngine has no Qt global; a point literal is coerced to QPoint.
fn class_at_coord_script(x: i32, y: i32) -> String {
    target_script(
        &format!("(ws.windowAt ? ws.windowAt({{x: {x}, y: {y}}}, 1) : [])[0]"),
        false,
    )
}

/// Returns the pid of the window owning the given window-UUID, used by
/// `inspect target=window:<ref>` to map window → AT-SPI application.
pub async fn pid_for_window_uuid(state: &KdeMcp, raw_uuid: &str) -> Result<Option<i64>, ToolError> {
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
        return {{ pid: found ? found.pid : -1 }};
        "#
    );
    let payload = run_script(&state.bus, &body, Duration::from_secs(2)).await?;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("pid_for_window_uuid JSON parse: {e}"),
        })?;
    Ok(v.get("pid").and_then(|p| p.as_i64()).filter(|p| *p > 0))
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
        Some(c) if !c.is_empty() => TargetApp::Class(c),
        _ => TargetApp::Classless,
    }
}

#[cfg(test)]
mod tests {
    use super::class_at_coord_script;

    #[test]
    fn changed_window_of_same_app_is_rejected() {
        let before: super::TargetSnapshot = serde_json::from_value(serde_json::json!({
            "class": "kate", "uuid": "window-a", "geometry": [0, 0, 800, 600]
        }))
        .unwrap();
        let after: super::TargetSnapshot = serde_json::from_value(serde_json::json!({
            "class": "kate", "uuid": "window-b", "geometry": [0, 0, 800, 600]
        }))
        .unwrap();
        assert!(before.ensure_unchanged(&after).is_err());
        assert!(before.ensure_unchanged(&before).is_ok());
    }

    #[test]
    fn moved_window_or_pointer_is_rejected() {
        let before: super::TargetSnapshot = serde_json::from_value(serde_json::json!({
            "class": "kate", "uuid": "window-a", "geometry": [0, 0, 800, 600], "cursor": [100, 100]
        }))
        .unwrap();
        let moved: super::TargetSnapshot = serde_json::from_value(serde_json::json!({
            "class": "kate", "uuid": "window-a", "geometry": [100, 0, 800, 600], "cursor": [100, 100]
        })).unwrap();
        let pointer: super::TargetSnapshot = serde_json::from_value(serde_json::json!({
            "class": "kate", "uuid": "window-a", "geometry": [0, 0, 800, 600], "cursor": [200, 100]
        }))
        .unwrap();
        assert!(before.ensure_unchanged(&moved).is_err());
        assert!(before.ensure_unchanged(&pointer).is_err());
    }

    /// Regression guard: KWin Plasma 6's QJSEngine has no `Qt` global, so
    /// the script must not reference it. See the function-level doc-comment
    /// on `class_at_coord_script` for the incident.
    #[test]
    fn class_at_coord_script_avoids_qt_global() {
        let s = class_at_coord_script(123, 456);
        assert!(
            !s.contains("Qt."),
            "script must not reference the Qt global (KWin Plasma 6 QJSEngine has no Qt): {s}"
        );
        assert!(s.contains("windowAt"), "script must still call windowAt");
        assert!(s.contains("123"), "x coord must be embedded in the script");
        assert!(s.contains("456"), "y coord must be embedded in the script");
    }
}
