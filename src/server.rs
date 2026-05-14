//! The rmcp wire surface.
//!
//! Per Phase 0 Spike 0.2, the `#[tool_router]` proc-macro does NOT compose
//! across multiple `impl` blocks (it emits a duplicate `tool_router()`
//! constructor → E0592). The 16 wire-named tools therefore live in one
//! `#[tool_router] impl KdeMcp` block below. Comment dividers preserve the
//! four-category grouping the spec §6 catalogue uses.
//!
//! Phase 1: every dispatch routes through `dispatch_audited`, which serialises
//! the args (with redaction), runs the tool future, times it, writes one
//! `AuditRecord` per call, and converts the `Result<T, ToolError>` into the
//! structured `CallToolResult` shape per spec §8.

use crate::audit::AuditLog;
use crate::cli::Mode;
use crate::error::ToolError;
use crate::refs::RefStore;
use crate::tools;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct KdeMcp {
    pub mode: Mode,
    pub refs: Arc<RefStore>,
    pub audit: Arc<AuditLog>,
    pub bus: zbus::Connection,
    pub atspi: Arc<crate::atspi::connection::AtspiConnection>,
    pub policy: crate::policy::SharedPolicyGate,
    pub bash_session: Arc<std::sync::Mutex<Option<crate::tools::bash::BashSession>>>,
    pub tray: Arc<crate::tray::TrayState>,
    pub tool_router: ToolRouter<KdeMcp>,
}

impl KdeMcp {
    pub fn new(mode: Mode, audit: Arc<AuditLog>, bus: zbus::Connection) -> Self {
        let cancel = tokio_util::sync::CancellationToken::new();
        Self::new_with_cancel(mode, audit, bus, cancel)
    }

    /// Variant used by `src/main.rs` to install the cancel-shortcut token.
    pub fn new_with_cancel(
        mode: Mode,
        audit: Arc<AuditLog>,
        bus: zbus::Connection,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Self {
        let config = crate::policy::PolicyConfig::load_default().unwrap_or_default();
        let policy = Arc::new(crate::policy::PolicyGate::new(config, cancel.clone()));
        let tray = crate::tray::TrayState::new(cancel);
        Self {
            mode,
            refs: Arc::new(RefStore::new(Duration::from_secs(60))),
            audit,
            bus,
            atspi: Arc::new(crate::atspi::connection::AtspiConnection::default()),
            policy,
            bash_session: Arc::new(std::sync::Mutex::new(None)),
            tray,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl KdeMcp {
    // ---- Observation (read-only) ----

    #[tool(
        description = "Situational snapshot of the desktop. No screenshot. Should be the reflex first call when entering an unfamiliar state.",
        annotations(read_only_hint = true)
    )]
    async fn observe(
        &self,
        Parameters(args): Parameters<tools::observe::ObserveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "observe", v, async move {
            tools::observe::observe(self, args).await
        })
        .await)
    }

    #[tool(
        description = "Drill into the AT-SPI tree of a window or element. Returns a flattened element list.",
        annotations(read_only_hint = true)
    )]
    async fn inspect(
        &self,
        Parameters(args): Parameters<tools::inspect::InspectArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "inspect", v, async move {
            tools::inspect::inspect(self, args).await
        })
        .await)
    }

    #[tool(
        description = "Capture pixels. Prefer observe/inspect for structural questions. Use a targeted region over 'full' for token efficiency.",
        annotations(read_only_hint = true)
    )]
    async fn screenshot(
        &self,
        Parameters(args): Parameters<tools::screenshot::ScreenshotArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "screenshot", v, async move {
            tools::screenshot::screenshot(self, args).await
        })
        .await)
    }

    #[tool(
        description = "OCR a region. Use when AT-SPI doesn't expose text (canvas, PDF, image). Output is always trust: external.",
        annotations(read_only_hint = true)
    )]
    async fn read_text(
        &self,
        Parameters(args): Parameters<tools::read_text::ReadTextArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "read_text", v, async move {
            tools::read_text::read_text(self, args).await
        })
        .await)
    }

    #[tool(
        description = "Block until a structured condition (window/element appears, screen changes, etc.). Replaces arbitrary sleeps.",
        annotations(read_only_hint = true)
    )]
    async fn wait(
        &self,
        Parameters(args): Parameters<tools::wait::WaitArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "wait", v, async move {
            tools::wait::wait(self, args).await
        })
        .await)
    }

    // ---- Action (destructive) ----

    #[tool(
        description = "Invoke a named action on an accessibility element by ref.",
        annotations(destructive_hint = true)
    )]
    async fn act_on_element(
        &self,
        Parameters(args): Parameters<tools::act_on_element::ActOnElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "act_on_element", v, async move {
                tools::act_on_element::act_on_element(self, args).await
            })
            .await,
        )
    }

    #[tool(
        description = "Mouse click/move/scroll/drag at coordinates. Vision fallback when AT-SPI is empty.",
        annotations(destructive_hint = true)
    )]
    async fn mouse(
        &self,
        Parameters(args): Parameters<tools::mouse::MouseArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "mouse", v, async move {
            tools::mouse::mouse(self, args).await
        })
        .await)
    }

    #[tool(
        name = "keyboard.type",
        description = "Type literal text. No modifier keys (use keyboard.key for chords).",
        annotations(destructive_hint = true)
    )]
    async fn keyboard_type(
        &self,
        Parameters(args): Parameters<tools::keyboard::KeyboardTypeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "keyboard.type", v, async move {
                tools::keyboard::keyboard_type(self, args).await
            })
            .await,
        )
    }

    #[tool(
        name = "keyboard.key",
        description = "Press named keys or chords (e.g. 'ctrl+s'). No text (use keyboard.type for text).",
        annotations(destructive_hint = true)
    )]
    async fn keyboard_key(
        &self,
        Parameters(args): Parameters<tools::keyboard::KeyboardKeyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "keyboard.key", v, async move {
                tools::keyboard::keyboard_key(self, args).await
            })
            .await,
        )
    }

    // ---- Window + clipboard (mixed) ----

    #[tool(
        description = "List windows on the desktop with class, title, geometry, screen.",
        annotations(read_only_hint = true)
    )]
    async fn window_query(
        &self,
        Parameters(args): Parameters<tools::window::WindowQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "window_query", v, async move {
                tools::window::window_query(self, args).await
            })
            .await,
        )
    }

    #[tool(
        description = "Focus/minimise/maximise/close a window by ref.",
        annotations(destructive_hint = true)
    )]
    async fn window_control(
        &self,
        Parameters(args): Parameters<tools::window::WindowControlArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "window_control", v, async move {
                tools::window::window_control(self, args).await
            })
            .await,
        )
    }

    #[tool(
        description = "Read the clipboard.",
        annotations(read_only_hint = true)
    )]
    async fn clipboard_read(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardReadArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "clipboard_read", v, async move {
                tools::clipboard::clipboard_read(self, args).await
            })
            .await,
        )
    }

    #[tool(
        description = "Set the clipboard contents.",
        annotations(destructive_hint = true)
    )]
    async fn clipboard_write(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardWriteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "clipboard_write", v, async move {
                tools::clipboard::clipboard_write(self, args).await
            })
            .await,
        )
    }

    // ---- Shell + filesystem ----

    #[tool(
        description = "Run a shell command via a persistent bash session.",
        annotations(destructive_hint = true, open_world_hint = true)
    )]
    async fn bash(
        &self,
        Parameters(args): Parameters<tools::bash::BashArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(dispatch_audited(audit, tray, mode, "bash", v, async move {
            tools::bash::bash(self, args).await
        })
        .await)
    }

    #[tool(
        name = "text_editor.view",
        description = "Read a file or directory listing (absolute path required).",
        annotations(read_only_hint = true)
    )]
    async fn text_editor_view(
        &self,
        Parameters(args): Parameters<tools::text_editor::TextEditorViewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "text_editor.view", v, async move {
                tools::text_editor::text_editor_view(self, args).await
            })
            .await,
        )
    }

    #[tool(
        name = "text_editor.edit",
        description = "Edit a file via create/replace/insert/undo (absolute path required).",
        annotations(destructive_hint = true)
    )]
    async fn text_editor_edit(
        &self,
        Parameters(args): Parameters<tools::text_editor::TextEditorEditArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        Ok(
            dispatch_audited(audit, tray, mode, "text_editor.edit", v, async move {
                tools::text_editor::text_editor_edit(self, args).await
            })
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for KdeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            // from_build_env() reads the SDK's own metadata, not ours — use
            // CARGO_PKG_NAME / CARGO_PKG_VERSION from the consuming crate so the
            // wire reports kde-mcp/0.1.0, not rmcp/1.7.0.
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_instructions("kde-mcp: KDE Plasma 6 / Wayland desktop control.".to_string())
    }
}

/// Wrap a tool future with audit-log dispatch.
///
/// - Serialises args to JSON, runs the redactor, writes the AuditRecord.
/// - Times the call to ms.
/// - On error, records `ok: false` and the structured error payload.
/// - The return type is `CallToolResult` so the rmcp `#[tool]` macro can
///   `?`-bubble it.
pub(crate) async fn dispatch_audited<F, T>(
    audit: Arc<AuditLog>,
    tray: Arc<crate::tray::TrayState>,
    mode: &'static str,
    tool: &'static str,
    args: serde_json::Value,
    fut: F,
) -> CallToolResult
where
    F: std::future::Future<Output = Result<T, ToolError>>,
    T: serde::Serialize,
{
    let _guard = crate::tray::TrayGuard::enter(&tray, tool);
    let started = std::time::Instant::now();
    let outcome = fut.await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let redacted_args = crate::audit::redact(tool, args);
    let (call_result, audit_error) = match outcome {
        Ok(payload) => {
            let v = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
            (
                CallToolResult::success(vec![Content::text(v.to_string())]),
                None,
            )
        }
        Err(e) => {
            let code = e.code();
            let msg = e.to_string();
            let details = serde_json::to_value(&e).unwrap_or(serde_json::Value::Null);
            let payload = serde_json::json!({ "code": code, "details": details });
            let r =
                CallToolResult::error(vec![Content::text(msg), Content::text(payload.to_string())]);
            (r, Some(payload))
        }
    };
    let rec = crate::audit::AuditRecord {
        ts: String::new(),
        mode,
        tool: tool.to_string(),
        args: redacted_args,
        ok: audit_error.is_none(),
        duration_ms: Some(duration_ms),
        error: audit_error,
        vision_fallback: None,
    };
    if let Err(e) = audit.write(rec).await {
        tracing::error!(error = %e, tool, "audit write failed");
    }
    call_result
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_audit() -> (tempfile::TempDir, Arc<AuditLog>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let audit = Arc::new(AuditLog::open(&path).await.unwrap());
        (dir, audit)
    }

    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_success() {
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel);
        let r = dispatch_audited(
            audit.clone(),
            tray,
            "live",
            "observe",
            serde_json::json!({"detail": "summary"}),
            async { Ok::<_, ToolError>(serde_json::json!({"mode":"live"})) },
        )
        .await;
        assert!(matches!(r.is_error, None | Some(false)));
        // Drop the AuditLog so the write is flushed and the file released.
        drop(audit);
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["tool"], "observe");
        assert_eq!(v["mode"], "live");
        assert_eq!(v["ok"], true);
        assert!(v["duration_ms"].is_number());
    }

    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_error() {
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel);
        let r = dispatch_audited(
            audit.clone(),
            tray,
            "live",
            "clipboard_write",
            serde_json::json!({"content": "secret"}),
            async { Err::<serde_json::Value, _>(ToolError::NotImplemented) },
        )
        .await;
        assert_eq!(r.is_error, Some(true));
        drop(audit);
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], "not_implemented");
        // Redaction: clipboard_write.content should be hashed in the audit record.
        assert!(
            v["args"]["content"]["sha256"].is_string(),
            "args.content should have been redacted: {v}",
        );
        assert!(
            !v.to_string().contains("\"secret\""),
            "secret leaked into audit"
        );
    }
}
