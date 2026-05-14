use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WaitArgs {
    /// "seconds" | "window_appears" | "window_disappears"
    /// | "screen_change" (Phase 6) | "element_appears" (Phase 6)
    pub condition: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub r#match: Option<WindowMatch>,
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
        "screen_change" | "element_appears" | "element_disappears" | "element_state" => {
            Err(ToolError::NotImplemented)
        }
        other => Err(ToolError::UnsupportedRegion {
            reason: format!("unknown wait condition: {other}"),
        }),
    }
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
