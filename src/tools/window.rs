use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::refs::RefKind;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryArgs {
    /// "list" | "get_geometry" | "find"
    #[serde(default)]
    pub action: Option<String>,
    /// "ids" | "titles" | "full"
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub r#match: Option<WindowMatch>,
    #[serde(default)]
    pub window_uuid: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WindowMatch {
    #[serde(default)]
    pub title_regex: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowEntry {
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Tagged<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<Tagged<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<[i32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryResponse {
    pub windows: Vec<WindowEntry>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WindowControlArgs {
    pub window_uuid: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<[i32; 4]>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WindowAction {
    Focus,
    Minimize,
    Maximize,
    Restore,
    Close,
    Move([i32; 4]),
    Resize([i32; 4]),
}

/// Validate the (action, geometry) pair from the wire and lift it into a
/// strongly-typed `WindowAction`.
///
/// Why: per spec §4.5 (poka-yoke), invalid combinations — move/resize without
/// geometry, or geometry on a non-spatial action — must surface as errors, not
/// silent no-ops the agent will mistake for success.
pub(crate) fn parse_action(
    action: &str,
    geometry: Option<[i32; 4]>,
) -> Result<WindowAction, ToolError> {
    let normalised = action.to_ascii_lowercase();
    match (normalised.as_str(), geometry) {
        ("move", Some(g)) => Ok(WindowAction::Move(g)),
        ("move", None) => Err(ToolError::UnsupportedRegion {
            reason: "geometry [x, y, w, h] required for action=move".into(),
        }),
        ("resize", Some(g)) => Ok(WindowAction::Resize(g)),
        ("resize", None) => Err(ToolError::UnsupportedRegion {
            reason: "geometry [x, y, w, h] required for action=resize".into(),
        }),
        (a @ ("focus" | "minimize" | "maximize" | "restore" | "close"), Some(_)) => {
            Err(ToolError::UnsupportedRegion {
                reason: format!("geometry is only valid for move/resize, not action={a}"),
            })
        }
        ("focus", None) => Ok(WindowAction::Focus),
        ("minimize", None) => Ok(WindowAction::Minimize),
        ("maximize", None) => Ok(WindowAction::Maximize),
        ("restore", None) => Ok(WindowAction::Restore),
        ("close", None) => Ok(WindowAction::Close),
        (other, _) => Err(ToolError::UnsupportedRegion {
            reason: format!("unknown window_control action: {other}"),
        }),
    }
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowControlResponse {
    pub ok: bool,
}

pub async fn window_query(
    state: &KdeMcp,
    args: WindowQueryArgs,
) -> Result<WindowQueryResponse, ToolError> {
    let format = args.format.as_deref().unwrap_or("titles").to_string();
    let action = args.action.as_deref().unwrap_or("list").to_string();
    let payload = run_script(&state.bus, JS_LIST_ALL, Duration::from_secs(2)).await?;
    let raw: Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("kwin script JSON parse: {e}"),
        })?;
    let mut entries = parse_entries(&raw)?;

    // Mint refs and replace the raw KWin UUID with our opaque ref.
    for entry in entries.iter_mut() {
        let opaque = state.refs.mint(RefKind::Window, entry.uuid.clone()).await;
        entry.uuid = opaque;
    }

    // Apply match filter.
    if let Some(m) = &args.r#match {
        if let Some(class) = &m.class {
            entries.retain(|e| e.class.as_ref().map(|c| &c.text == class).unwrap_or(false));
        }
        if let Some(tr) = &m.title_regex {
            let re = regex_lite::Regex::new(tr).map_err(|e| ToolError::UnsupportedRegion {
                reason: format!("invalid title_regex: {e}"),
            })?;
            entries.retain(|e| {
                e.title
                    .as_ref()
                    .map(|t| re.is_match(&t.text))
                    .unwrap_or(false)
            });
        }
        if let Some(pid) = m.pid {
            entries.retain(|e| e.pid == Some(pid));
        }
    }

    match action.as_str() {
        "list" => { /* no extra filter */ }
        "get_geometry" => {
            let ref_id = args
                .window_uuid
                .ok_or_else(|| ToolError::UnsupportedRegion {
                    reason: "window_uuid required for action=get_geometry".into(),
                })?;
            entries.retain(|e| e.uuid == ref_id);
            if entries.is_empty() {
                return Err(ToolError::TargetNotFound {
                    closest_match: Some(format!("no window matches ref {ref_id}")),
                });
            }
            // get_geometry always returns the full shape regardless of
            // requested format — the caller asked for geometry specifically.
            return Ok(WindowQueryResponse { windows: entries });
        }
        "find" => {
            // find returns just the first match after applying the filters
            // above. If no filters were given, returns the first window.
            entries.truncate(1);
            if entries.is_empty() {
                return Err(ToolError::TargetNotFound {
                    closest_match: Some("no windows match the supplied filter".into()),
                });
            }
        }
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unknown window_query action: {other}"),
            });
        }
    }

    let projected = project(&entries, &format);
    Ok(WindowQueryResponse { windows: projected })
}

pub(crate) fn project(entries: &[WindowEntry], format: &str) -> Vec<WindowEntry> {
    entries
        .iter()
        .map(|e| match format {
            "ids" => WindowEntry {
                uuid: e.uuid.clone(),
                title: None,
                class: e.class.clone(),
                pid: None,
                geometry: None,
                screen: e.screen.clone(),
                minimized: None,
                maximized: None,
                fullscreen: None,
            },
            "titles" => WindowEntry {
                uuid: e.uuid.clone(),
                title: e.title.clone(),
                class: e.class.clone(),
                pid: None,
                geometry: None,
                screen: e.screen.clone(),
                minimized: None,
                maximized: None,
                fullscreen: None,
            },
            _ => e.clone(),
        })
        .collect()
}

pub(crate) fn parse_entries(v: &Value) -> Result<Vec<WindowEntry>, ToolError> {
    let arr = v
        .as_array()
        .ok_or_else(|| ToolError::ExternalCommandFailed {
            stderr: "kwin script payload was not a JSON array".into(),
        })?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let g = item.get("geometry").and_then(|g| g.as_array());
        let geometry = g.and_then(|a| {
            if a.len() == 4 {
                Some([
                    a[0].as_i64().unwrap_or(0) as i32,
                    a[1].as_i64().unwrap_or(0) as i32,
                    a[2].as_i64().unwrap_or(0) as i32,
                    a[3].as_i64().unwrap_or(0) as i32,
                ])
            } else {
                None
            }
        });
        out.push(WindowEntry {
            uuid: item
                .get("uuid")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            title: item
                .get("caption")
                .and_then(|x| x.as_str())
                .map(|s| Tagged {
                    trust: Trust::External,
                    text: s.to_string(),
                }),
            class: item
                .get("resourceClass")
                .and_then(|x| x.as_str())
                .map(|s| Tagged {
                    trust: Trust::Trusted,
                    text: s.to_string(),
                }),
            pid: item.get("pid").and_then(|x| x.as_i64()),
            geometry,
            screen: item
                .get("screen")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            minimized: item.get("minimized").and_then(|x| x.as_bool()),
            maximized: item.get("maximized").and_then(|x| x.as_bool()),
            fullscreen: item.get("fullScreen").and_then(|x| x.as_bool()),
        });
    }
    Ok(out)
}

pub(crate) const JS_LIST_ALL: &str = r#"
    var ws = workspace;
    var windows = ws.windowList();
    return windows.map(function(w) {
        var g = w.frameGeometry;
        var out = w.output;
        return {
            uuid: w.internalId ? w.internalId.toString() : "",
            caption: w.caption || "",
            resourceClass: w.resourceClass || "",
            pid: w.pid || 0,
            geometry: [g.x, g.y, g.width, g.height],
            screen: out ? out.name : null,
            minimized: !!w.minimized,
            fullScreen: !!w.fullScreen,
            maximized: w.maximizeMode ? (w.maximizeMode > 0) : false
        };
    });
"#;

pub async fn window_control(
    state: &KdeMcp,
    args: WindowControlArgs,
) -> Result<WindowControlResponse, ToolError> {
    let action = parse_action(&args.action, args.geometry)?;

    // Resolve our opaque ref → KWin internalId UUID.
    let meta = state
        .refs
        .resolve(&args.window_uuid)
        .await
        .map_err(|_| ToolError::StaleHandle {
            ref_id: args.window_uuid.clone(),
        })?;
    if meta.kind != crate::refs::RefKind::Window {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("ref {} is not a window", args.window_uuid),
        });
    }
    let kwin_uuid = meta.payload.clone();

    // Policy gate.
    let target_class = crate::policy_target::class_for_window_uuid(state, &kwin_uuid).await?;
    let _policy_warnings = state
        .policy
        .check(
            crate::policy_target::to_target(target_class),
            crate::policy::Action::Generic,
        )
        .await?;

    let body = match action {
        WindowAction::Focus => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ ws.activeWindow = list[i]; break; }} }} return {{ok: true}};"#
        ),
        WindowAction::Minimize => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ list[i].minimized = true; break; }} }} return {{ok: true}};"#
        ),
        WindowAction::Maximize => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ list[i].setMaximize(true, true); break; }} }} return {{ok: true}};"#
        ),
        WindowAction::Restore => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ list[i].minimized = false; list[i].setMaximize(false, false); break; }} }} return {{ok: true}};"#
        ),
        WindowAction::Close => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ list[i].closeWindow(); break; }} }} return {{ok: true}};"#
        ),
        WindowAction::Move([x, y, w, h]) | WindowAction::Resize([x, y, w, h]) => format!(
            r#"var ws = workspace; var list = ws.windowList(); for (var i=0;i<list.length;i++) {{ if (list[i].internalId && list[i].internalId.toString() === "{kwin_uuid}") {{ list[i].frameGeometry = Qt.rect({x}, {y}, {w}, {h}); break; }} }} return {{ok: true}};"#
        ),
    };
    let _ =
        crate::kwin::scripting::run_script(&state.bus, &body, std::time::Duration::from_secs(2))
            .await?;
    Ok(WindowControlResponse { ok: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_ids_drops_everything_but_uuid_and_class() {
        let raw = json!([
            {"uuid":"win#1","caption":"Kate","resourceClass":"kate","pid":12,"geometry":[0,0,800,600],"screen":"DP-1"},
            {"uuid":"win#2","caption":"Brave","resourceClass":"brave-browser","pid":13,"geometry":[0,0,800,600],"screen":"DP-1"}
        ]);
        let entries = parse_entries(&raw).unwrap();
        let projected = project(&entries, "ids");
        let v = serde_json::to_value(&projected).unwrap();
        // `title` is omitted (None) and serde skips it; the object should not
        // have a `title` key at all.
        assert!(v[0].get("title").is_none());
        assert!(v[0]["uuid"].is_string());
        assert!(v[0]["class"].is_object());
    }

    #[test]
    fn format_titles_includes_caption_but_not_pid_or_geometry() {
        let raw = json!([
            {"uuid":"win#1","caption":"Kate","resourceClass":"kate","pid":12,"geometry":[0,0,800,600],"screen":"DP-1"}
        ]);
        let entries = parse_entries(&raw).unwrap();
        let projected = project(&entries, "titles");
        let v = serde_json::to_value(&projected).unwrap();
        assert!(v[0]["title"].is_object());
        assert!(v[0].get("pid").is_none());
        assert!(v[0].get("geometry").is_none());
    }

    #[test]
    fn parse_entries_extracts_geometry_array() {
        let raw = json!([
            {"uuid":"win#1","caption":"K","resourceClass":"k","pid":1,"geometry":[10,20,300,400],"screen":"DP-1"}
        ]);
        let entries = parse_entries(&raw).unwrap();
        assert_eq!(entries[0].geometry, Some([10, 20, 300, 400]));
        assert_eq!(entries[0].pid, Some(1));
    }

    #[test]
    fn window_control_args_accept_window_uuid_action_geometry() {
        let v = json!({
            "window_uuid": "r#1",
            "action": "move",
            "geometry": [10, 20, 300, 400],
        });
        let args: WindowControlArgs = serde_json::from_value(v).unwrap();
        assert_eq!(args.window_uuid, "r#1");
        assert_eq!(args.action, "move");
        assert_eq!(args.geometry, Some([10, 20, 300, 400]));
    }

    #[test]
    fn parse_action_move_with_geometry_ok() {
        let a = parse_action("move", Some([10, 20, 300, 400])).unwrap();
        assert_eq!(a, WindowAction::Move([10, 20, 300, 400]));
    }

    #[test]
    fn parse_action_move_without_geometry_errors() {
        let err = parse_action("move", None).unwrap_err();
        assert!(
            matches!(err, ToolError::UnsupportedRegion { ref reason } if reason.contains("geometry") && reason.contains("move"))
        );
    }

    #[test]
    fn parse_action_resize_with_geometry_ok() {
        let a = parse_action("resize", Some([0, 0, 800, 600])).unwrap();
        assert_eq!(a, WindowAction::Resize([0, 0, 800, 600]));
    }

    #[test]
    fn parse_action_resize_without_geometry_errors() {
        let err = parse_action("resize", None).unwrap_err();
        assert!(
            matches!(err, ToolError::UnsupportedRegion { ref reason } if reason.contains("geometry") && reason.contains("resize"))
        );
    }

    #[test]
    fn parse_action_focus_with_extra_geometry_errors() {
        // Poka-yoke (§4.5): geometry on a non-move/resize action should not
        // silently be ignored — the agent thinks the move happened. Reject.
        let err = parse_action("focus", Some([0, 0, 100, 100])).unwrap_err();
        assert!(
            matches!(err, ToolError::UnsupportedRegion { ref reason } if reason.contains("geometry") && reason.contains("focus"))
        );
    }

    #[test]
    fn parse_action_focus_ok() {
        assert_eq!(parse_action("focus", None).unwrap(), WindowAction::Focus);
    }

    #[test]
    fn parse_action_unknown_errors() {
        let err = parse_action("bogus", None).unwrap_err();
        assert!(
            matches!(err, ToolError::UnsupportedRegion { ref reason } if reason.contains("bogus"))
        );
    }
}
