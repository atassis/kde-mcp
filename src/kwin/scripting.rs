//! KWin Scripting loadScript runner — caller-hosted D-Bus receiver pattern.
//!
//! KWin's QJSEngine exposes no file I/O. The only escape hatch for a script
//! is `callDBus(service, path, interface, method, args...)`. This module
//! hosts a unique sink interface for the duration of each script invocation
//! and lets the script call back into it.

use crate::error::ToolError;
use std::time::Duration;
use tokio::sync::mpsc;
use zbus::Connection;

/// Render the JS body wrapped with the caller-callback prelude.
///
/// The body is expected to be a series of statements that ends with
/// `return <value>` (the IIFE captures the return value). The wrapper
/// JSON-stringifies the return value and sends it back as one
/// `result(payload: s)` D-Bus call; any thrown exception is caught and
/// turned into one `error(message: s)` call instead.
pub(crate) fn render_script(caller_bus_name: &str, sink_obj_path: &str, user_body: &str) -> String {
    format!(
        r#"
(function() {{
    function _send(method, payload) {{
        callDBus({caller_bus_name:?}, {sink_obj_path:?},
                 "org.kde.kdemcp.ScriptSink1", method, payload);
    }}
    try {{
        var _r = (function() {{
            {user_body}
        }})();
        _send("result", typeof _r === "string" ? _r : JSON.stringify(_r));
    }} catch (e) {{
        _send("error", String(e));
    }}
}})();
"#
    )
}

/// Run a KWin script body and return its JSON-encoded payload string.
pub async fn run_script(
    bus: &Connection,
    body: &str,
    timeout: Duration,
) -> Result<String, ToolError> {
    let invocation_id = uuid::Uuid::new_v4().simple().to_string();
    let obj_path = format!("/org/kde/kdemcp/Sink/{invocation_id}");
    let plugin_name = format!("kde-mcp-{invocation_id}");

    let caller_name = bus
        .unique_name()
        .ok_or_else(|| ToolError::BusDisconnected {
            detail: "no unique name on session bus".into(),
        })?
        .to_string();

    let (tx, mut rx) = mpsc::channel::<Result<String, String>>(2);

    // Object-server side: register an ad hoc interface for the duration.
    let sink = ScriptSink { tx };
    bus.object_server()
        .at(obj_path.clone(), sink)
        .await
        .map_err(busdc)?;

    // Tempfile with the rendered JS.
    let js = render_script(&caller_name, &obj_path, body);
    let mut tmp = tempfile::Builder::new()
        .prefix("kde-mcp-script-")
        .suffix(".js")
        .tempfile()
        .map_err(|e| ToolError::Io {
            path: "/tmp/kde-mcp-script-*.js".into(),
            error: e.to_string(),
        })?;
    use std::io::Write;
    tmp.write_all(js.as_bytes()).map_err(|e| ToolError::Io {
        path: tmp.path().to_string_lossy().to_string(),
        error: e.to_string(),
    })?;
    tmp.flush().map_err(|e| ToolError::Io {
        path: tmp.path().to_string_lossy().to_string(),
        error: e.to_string(),
    })?;

    // Load and run.
    let scripting_proxy = ScriptingProxy::new(bus).await.map_err(busdc)?;
    let script_id = scripting_proxy
        .load_script(&tmp.path().to_string_lossy(), &plugin_name)
        .await
        .map_err(busdc)?;
    let script_obj_path = format!("/Scripting/Script{script_id}");
    let script_proxy = ScriptProxy::builder(bus)
        .destination("org.kde.KWin")
        .map_err(busdc)?
        .path(script_obj_path)
        .map_err(busdc)?
        .build()
        .await
        .map_err(busdc)?;
    script_proxy.run().await.map_err(busdc)?;

    // Await first result/error or timeout.
    let outcome = match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(Ok(payload))) => Ok(payload),
        Ok(Some(Err(msg))) => Err(ToolError::ExternalCommandFailed { stderr: msg }),
        Ok(None) => Err(ToolError::ExternalCommandFailed {
            stderr: "script channel closed without delivering a result".into(),
        }),
        Err(_) => Err(ToolError::Timeout {
            closest_match: None,
            current_windows: vec![],
        }),
    };

    // Cleanup — best-effort, never overshadows the actual outcome.
    let _ = scripting_proxy.unload_script(&plugin_name).await;
    let _ = bus
        .object_server()
        .remove::<ScriptSink, _>(obj_path.as_str())
        .await;
    drop(tmp);

    outcome
}

fn busdc(e: zbus::Error) -> ToolError {
    ToolError::BusDisconnected {
        detail: e.to_string(),
    }
}

struct ScriptSink {
    tx: mpsc::Sender<Result<String, String>>,
}

#[zbus::interface(name = "org.kde.kdemcp.ScriptSink1")]
impl ScriptSink {
    async fn result(&self, payload: String) {
        let _ = self.tx.send(Ok(payload)).await;
    }
    async fn error(&self, message: String) {
        let _ = self.tx.send(Err(message)).await;
    }
}

#[zbus::proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
trait Scripting {
    fn load_script(&self, file_path: &str, plugin_name: &str) -> zbus::Result<i32>;
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.kde.kwin.Script",
    default_service = "org.kde.KWin",
    default_path = "/Scripting/Script0"
)]
trait Script {
    fn run(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_script_embeds_caller_name_path_and_body() {
        let js = render_script(
            ":1.123",
            "/org/kde/kdemcp/Sink/abcd",
            "return 42;",
        );
        assert!(js.contains("\":1.123\""), "missing caller bus name");
        assert!(js.contains("\"/org/kde/kdemcp/Sink/abcd\""), "missing obj path");
        assert!(js.contains("return 42;"), "missing user body");
        assert!(js.contains("(function()"), "body not wrapped in IIFE");
    }

    #[test]
    fn rendered_script_escapes_callbus_args_via_json_debug() {
        // The user body itself is NOT escaped — but the bus name and path
        // are inserted via `{:?}` which produces double-quoted JS strings.
        // A bus name with a quote in it would be escaped; bus names cannot
        // legally contain quotes (D-Bus spec), so this is a robustness
        // measure rather than a hot path.
        let js = render_script(r#":1."weird""#, "/path", "return 1;");
        assert!(js.contains(r#":1.\"weird\""#), "expected escaped name");
    }
}
