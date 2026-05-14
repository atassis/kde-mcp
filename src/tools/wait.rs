use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WaitArgs {
    /// "seconds" | "window_appears" | "window_disappears" | "screen_change"
    /// | "element_appears" | "element_disappears"
    pub condition: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub r#match: Option<WindowMatch>,
    /// For screen_change — pixel-fraction threshold. Default 0.02 (2%).
    #[serde(default)]
    pub pixel_threshold: Option<f32>,
    /// For element_* — role + name regex filter.
    #[serde(default)]
    pub element_match: Option<ElementMatch>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ElementMatch {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name_pattern: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct WindowMatch {
    #[serde(default)]
    pub title_regex: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub pid: Option<i64>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WaitResponse {
    pub matched: bool,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timed_out: Option<bool>,
}

pub async fn wait(state: &KdeMcp, args: WaitArgs) -> Result<WaitResponse, ToolError> {
    let started = Instant::now();
    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(5000) as u64);
    let condition = args.condition.to_ascii_lowercase();
    match condition.as_str() {
        "seconds" => {
            let s = args.seconds.unwrap_or(0.0).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(s)).await;
            Ok(WaitResponse {
                matched: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: None,
            })
        }
        "window_appears" => poll_window(state, args.r#match, timeout, started, true).await,
        "window_disappears" => poll_window(state, args.r#match, timeout, started, false).await,
        "screen_change" => {
            poll_screen_change(
                state,
                args.pixel_threshold.unwrap_or(0.02),
                timeout,
                started,
            )
            .await
        }
        "element_appears" => poll_element(state, args.element_match, timeout, started, true).await,
        "element_disappears" => {
            poll_element(state, args.element_match, timeout, started, false).await
        }
        "element_state" => Err(ToolError::NotImplemented),
        other => Err(ToolError::UnsupportedRegion {
            reason: format!("unknown wait condition: {other}"),
        }),
    }
}

async fn poll_screen_change(
    state: &KdeMcp,
    threshold: f32,
    timeout: Duration,
    started: Instant,
) -> Result<WaitResponse, ToolError> {
    use crate::kwin::screenshot::{CaptureRequest, Region, capture};
    let initial = capture(
        &state.bus,
        CaptureRequest {
            region: Region::ActiveScreen,
            include_cursor: false,
        },
    )
    .await?;
    let initial_rgba = initial.image.to_rgba8();
    let total_px = (initial_rgba.width() as u64) * (initial_rgba.height() as u64);
    let deadline = started + timeout;
    loop {
        if Instant::now() >= deadline {
            return Ok(WaitResponse {
                matched: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: Some(true),
            });
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let frame = capture(
            &state.bus,
            CaptureRequest {
                region: Region::ActiveScreen,
                include_cursor: false,
            },
        )
        .await?;
        let frame_rgba = frame.image.to_rgba8();
        if frame_rgba.dimensions() != initial_rgba.dimensions() {
            // resolution changed → definitely a screen_change
            return Ok(WaitResponse {
                matched: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: None,
            });
        }
        // Count pixels that differ by >16 in any channel. Fast enough at
        // 5 fps for typical 2K displays (~4M px × 4 byte cmp).
        let mut diff = 0u64;
        for (a, b) in initial_rgba
            .pixels()
            .zip(frame_rgba.pixels())
        {
            if (a[0] as i32 - b[0] as i32).abs() > 16
                || (a[1] as i32 - b[1] as i32).abs() > 16
                || (a[2] as i32 - b[2] as i32).abs() > 16
            {
                diff += 1;
            }
        }
        let frac = diff as f32 / total_px.max(1) as f32;
        if frac >= threshold {
            return Ok(WaitResponse {
                matched: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: None,
            });
        }
    }
}

async fn poll_element(
    state: &KdeMcp,
    elem_match: Option<ElementMatch>,
    timeout: Duration,
    started: Instant,
    expect_present: bool,
) -> Result<WaitResponse, ToolError> {
    let conn = state.atspi.get().await?;
    let role_filter = elem_match.as_ref().and_then(|m| m.role.clone());
    let name_re = elem_match
        .as_ref()
        .and_then(|m| m.name_pattern.as_deref())
        .map(regex_lite::Regex::new)
        .transpose()
        .map_err(|e| ToolError::UnsupportedRegion {
            reason: format!("name_pattern: {e}"),
        })?;
    let deadline = started + timeout;
    loop {
        if Instant::now() >= deadline {
            return Ok(WaitResponse {
                matched: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: Some(true),
            });
        }
        let found = scan_for_element(conn, role_filter.as_deref(), name_re.as_ref()).await;
        if found == expect_present {
            return Ok(WaitResponse {
                matched: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: None,
            });
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn scan_for_element(
    conn: &atspi::AccessibilityConnection,
    role_filter: Option<&str>,
    name_re: Option<&regex_lite::Regex>,
) -> bool {
    use atspi::proxy::accessible::ObjectRefExt;
    use std::collections::VecDeque;
    let root = match conn.root_accessible_on_registry().await {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut queue: VecDeque<atspi::proxy::accessible::AccessibleProxy<'_>> = VecDeque::new();
    queue.push_back(root);
    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if visited > 500 {
            break;
        }
        let role = node
            .get_role()
            .await
            .ok()
            .map(|r| r.name().to_string())
            .unwrap_or_default();
        let name = node.name().await.unwrap_or_default();
        let role_ok = role_filter
            .map(|r| role.eq_ignore_ascii_case(r))
            .unwrap_or(true);
        let name_ok = name_re.map(|re| re.is_match(&name)).unwrap_or(true);
        if role_ok && name_ok && role_filter.is_some() {
            return true;
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
    false
}

async fn poll_window(
    state: &KdeMcp,
    match_: Option<WindowMatch>,
    timeout: Duration,
    started: Instant,
    expect_present: bool,
) -> Result<WaitResponse, ToolError> {
    let deadline = started + timeout;
    let re = match_
        .as_ref()
        .and_then(|m| m.title_regex.as_deref())
        .map(regex_lite::Regex::new)
        .transpose()
        .map_err(|e| ToolError::UnsupportedRegion {
            reason: format!("title_regex: {e}"),
        })?;
    let class_filter = match_.as_ref().and_then(|m| m.class.clone());
    let pid_filter = match_.as_ref().and_then(|m| m.pid);
    loop {
        let payload = crate::kwin::scripting::run_script(
            &state.bus,
            crate::tools::window::JS_LIST_ALL,
            Duration::from_secs(2),
        )
        .await?;
        let v: serde_json::Value =
            serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
        let entries = crate::tools::window::parse_entries(&v).unwrap_or_default();
        let any_matches = entries.iter().any(|e| {
            let class_ok = class_filter
                .as_deref()
                .map(|c| e.class.as_ref().map(|x| x.text == c).unwrap_or(false))
                .unwrap_or(true);
            let title_ok = re
                .as_ref()
                .map(|r| {
                    e.title
                        .as_ref()
                        .map(|t| r.is_match(&t.text))
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            let pid_ok = pid_filter.map(|p| e.pid == Some(p)).unwrap_or(true);
            class_ok && title_ok && pid_ok
        });
        if any_matches == expect_present {
            return Ok(WaitResponse {
                matched: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: None,
            });
        }
        if Instant::now() >= deadline {
            return Ok(WaitResponse {
                matched: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                timed_out: Some(true),
            });
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}
