use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::refs::RefKind;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ObserveArgs {
    #[serde(default)]
    pub detail: Option<DetailLevel>,
    #[serde(default)]
    pub include_other_windows: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Summary,
    #[default]
    Interactive,
    Full,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ObserveResponse {
    pub mode: String,
    pub cursor: [i32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_window: Option<FocusedWindow>,
    /// Phase 1 first cut: null with a warning. Background focus-tracking via
    /// AT-SPI `FocusEvents` lands in Phase 4 polish (spec §15 lists this as
    /// open work — focus events require a long-running signal subscription
    /// rather than a one-shot query, which Phase 1's request/response shape
    /// can't accommodate without scope creep).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_element: Option<()>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<crate::tools::window::WindowEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screens: Option<Vec<Screen>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FocusedWindow {
    pub uuid: String,
    pub title: Tagged<String>,
    pub class: Tagged<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<[i32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Screen {
    pub name: String,
    pub geometry: [i32; 4],
    pub scale: f32,
    pub primary: bool,
}

pub async fn observe(state: &KdeMcp, args: ObserveArgs) -> Result<ObserveResponse, ToolError> {
    let detail = args.detail.unwrap_or_default();
    let include_others = args.include_other_windows.unwrap_or(true);

    let payload = run_script(&state.bus, JS_OBSERVE, Duration::from_secs(2)).await?;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("observe script parse: {e}"),
        })?;

    let cursor = [
        v.pointer("/cursor/0").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
        v.pointer("/cursor/1").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
    ];

    let mut focused_window: Option<FocusedWindow> = None;
    if let Some(fw) = v.get("focused").filter(|x| !x.is_null()) {
        let uuid_raw = fw
            .get("uuid")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let opaque = state.refs.mint(RefKind::Window, uuid_raw).await;
        focused_window = Some(FocusedWindow {
            uuid: opaque,
            title: Tagged {
                trust: Trust::External,
                text: fw
                    .get("caption")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .into(),
            },
            class: Tagged {
                trust: Trust::Trusted,
                text: fw
                    .get("resourceClass")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .into(),
            },
            pid: fw.get("pid").and_then(|x| x.as_i64()),
            geometry: fw.get("geometry").and_then(|g| {
                g.as_array().filter(|a| a.len() == 4).map(|a| {
                    [
                        a[0].as_i64().unwrap_or(0) as i32,
                        a[1].as_i64().unwrap_or(0) as i32,
                        a[2].as_i64().unwrap_or(0) as i32,
                        a[3].as_i64().unwrap_or(0) as i32,
                    ]
                })
            }),
            screen: fw
                .get("screen")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            minimized: fw
                .get("minimized")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            maximized: fw
                .get("maximized")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            fullscreen: fw
                .get("fullScreen")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        });
    }

    let mut windows = None;
    let mut screens = None;
    let mut warnings = Vec::new();

    match detail {
        DetailLevel::Summary => {
            // Per spec §6.1: in summary mode, return just a count of other
            // windows when include_other_windows is true (the default).
            // When it's false, drop the windows info entirely (no warning).
            if include_others
                && let Some(arr) = v.get("windows").and_then(|w| w.as_array())
                && !arr.is_empty()
            {
                warnings.push(format!(
                    "{} other windows present (use detail=interactive for details)",
                    arr.len()
                ));
            }
        }
        DetailLevel::Interactive | DetailLevel::Full => {
            if let Some(arr) = v.get("windows").and_then(|w| w.as_array()) {
                let val = serde_json::Value::Array(arr.clone());
                let mut entries = crate::tools::window::parse_entries(&val)?;
                for entry in entries.iter_mut() {
                    let raw = entry.uuid.clone();
                    entry.uuid = state.refs.mint(RefKind::Window, raw).await;
                }
                windows = Some(entries);
            }
            if matches!(detail, DetailLevel::Full)
                && let Some(arr) = v.get("screens").and_then(|s| s.as_array())
            {
                let mut out = Vec::new();
                for s in arr {
                    let g = s.get("geometry").and_then(|g| g.as_array());
                    let geometry = if let Some(a) = g.filter(|a| a.len() == 4) {
                        [
                            a[0].as_i64().unwrap_or(0) as i32,
                            a[1].as_i64().unwrap_or(0) as i32,
                            a[2].as_i64().unwrap_or(0) as i32,
                            a[3].as_i64().unwrap_or(0) as i32,
                        ]
                    } else {
                        [0, 0, 0, 0]
                    };
                    out.push(Screen {
                        name: s
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        geometry,
                        scale: s
                            .get("scale")
                            .and_then(|x| x.as_f64())
                            .map(|x| x as f32)
                            .unwrap_or(1.0),
                        primary: s.get("primary").and_then(|x| x.as_bool()).unwrap_or(false),
                    });
                }
                screens = Some(out);
            }
        }
    }

    // Phase 1 first cut for focused_element: not implemented (see field doc).
    warnings.push(
        "focused_element omitted — focus-tracking via AT-SPI signals lands in Phase 4 polish"
            .to_string(),
    );

    Ok(ObserveResponse {
        mode: state.mode.as_str().to_string(),
        cursor,
        focused_window,
        focused_element: None,
        windows,
        screens,
        warnings,
    })
}

const JS_OBSERVE: &str = r#"
    var ws = workspace;
    var cur = ws.cursorPos;
    var active = ws.activeWindow;
    var others = ws.windowList() || [];
    var screens = (ws.screens || []).map(function(s) {
        return {
            name: s.name,
            geometry: [s.geometry.x, s.geometry.y, s.geometry.width, s.geometry.height],
            scale: s.scale,
            primary: (ws.activeScreen === s)
        };
    });
    function dump(w) {
        if (!w) return null;
        var g = w.frameGeometry;
        return {
            uuid: w.internalId ? w.internalId.toString() : "",
            caption: w.caption || "",
            resourceClass: w.resourceClass || "",
            pid: w.pid || 0,
            geometry: [g.x, g.y, g.width, g.height],
            screen: w.output ? w.output.name : null,
            minimized: !!w.minimized,
            fullScreen: !!w.fullScreen,
            maximized: w.maximizeMode ? (w.maximizeMode > 0) : false
        };
    }
    return {
        cursor: [cur.x, cur.y],
        focused: dump(active),
        windows: others.filter(function(w) { return w !== active; }).map(dump),
        screens: screens
    };
"#;
