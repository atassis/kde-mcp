//! The rmcp wire surface.
//!
//! Per Phase 0 Spike 0.2, the `#[tool_router]` proc-macro does NOT compose
//! across multiple `impl` blocks (it emits a duplicate `tool_router()`
//! constructor → E0592). The 12 wire-named tools (ADR-0013) therefore live
//! in one `#[tool_router] impl KdeMcp` block below. Comment dividers preserve
//! the three-category grouping the post-ADR-0013 catalogue uses.
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
    pub tray: Arc<crate::tray::TrayState>,
    pub lease: Arc<crate::lease::LeaseStore>,
    pub tool_router: ToolRouter<KdeMcp>,
    /// Daemon-mode session attribution (ADR-0012). Set by the rmcp
    /// factory closure per new connection; `None` in stdio mode and in
    /// any clone made before a session is assigned.
    pub session_id: Option<crate::transport::session::SessionId>,
    /// RAII guard whose `Drop` fires once rmcp lets go of the last
    /// clone of this per-session `KdeMcp`. Bookkeeping happens there:
    /// registry close, tray-session decrement, last-active stamp,
    /// `session_close` audit. `None` in stdio mode and for any clone
    /// minted before [`with_session_scope`] runs.
    pub _session_scope: Option<Arc<crate::transport::session::SessionScope>>,
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
        let lease = Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            uuid::Uuid::new_v4().to_string(),
        ));
        Self {
            mode,
            refs: Arc::new(RefStore::new(Duration::from_secs(60))),
            audit,
            bus,
            atspi: Arc::new(crate::atspi::connection::AtspiConnection::default()),
            policy,
            tray,
            lease,
            tool_router: Self::tool_router(),
            session_id: None,
            _session_scope: None,
        }
    }

    /// Return a clone of `self` tagged with the given session id. Used
    /// by the daemon's rmcp factory closure to give each MCP session a
    /// uniquely-attributed view of the server (audit, lease handoff).
    pub fn with_session_id(&self, id: crate::transport::session::SessionId) -> Self {
        let mut s = self.clone();
        s.session_id = Some(id);
        s
    }

    /// Same as [`with_session_id`] plus attaches a [`SessionScope`]
    /// guard whose `Drop` runs when rmcp finally lets go of this
    /// clone. The daemon uses this to detect real session-end timing
    /// (which the response-return-time middleware was racing past).
    pub fn with_session_scope(&self, scope: Arc<crate::transport::session::SessionScope>) -> Self {
        let mut s = self.with_session_id(scope.session_id());
        s._session_scope = Some(scope);
        s
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "observe",
            v,
            async move { tools::observe::observe(self, args).await },
        )
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "inspect",
            v,
            async move { tools::inspect::inspect(self, args).await },
        )
        .await)
    }

    #[tool(
        description = "Capture pixels as an MCP image-content block. Prefer observe/inspect for structural questions — pixels are the escape hatch. Region targeting: 'window:<ref>' or 'element:<ref>' are far cheaper than 'full' or 'active_screen'. Sizing via `max_long_edge` (default 1280): use 1024 for quick \"is X visible?\" probes, 1280 for UI-element identification, and stay ≤1568 (Anthropic's vision sweet spot — above it the client downsamples server-side and your vision tokens are wasted).",
        annotations(read_only_hint = true)
    )]
    async fn screenshot(
        &self,
        Parameters(args): Parameters<tools::screenshot::ScreenshotArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        // Routes the PNG back as an MCP image-content block (not as a
        // base64 string inside a JSON tool result) so Claude Code can
        // deliver the pixels into the model's vision pathway directly.
        Ok(dispatch_audited_with_content(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "screenshot",
            v,
            async move { tools::screenshot::screenshot(self, args).await },
        )
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "read_text",
            v,
            async move { tools::read_text::read_text(self, args).await },
        )
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "act_on_element",
            v,
            async move { tools::act_on_element::act_on_element(self, args).await },
        )
        .await)
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "mouse",
            v,
            async move { tools::mouse::mouse(self, args).await },
        )
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "keyboard.type",
            v,
            async move { tools::keyboard::keyboard_type(self, args).await },
        )
        .await)
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "keyboard.key",
            v,
            async move { tools::keyboard::keyboard_key(self, args).await },
        )
        .await)
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "window_query",
            v,
            async move { tools::window::window_query(self, args).await },
        )
        .await)
    }

    #[tool(
        description = "Mutate window state by uuid. Actions: focus | minimize | maximize | restore | close | move | resize. move/resize require geometry=[x,y,w,h]; other actions reject geometry.",
        annotations(destructive_hint = true)
    )]
    async fn window_control(
        &self,
        Parameters(args): Parameters<tools::window::WindowControlArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "window_control",
            v,
            async move { tools::window::window_control(self, args).await },
        )
        .await)
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Observe,
            mode,
            session_id.clone(),
            "clipboard_read",
            v,
            async move { tools::clipboard::clipboard_read(self, args).await },
        )
        .await)
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
        let session_id = self.session_id.map(|s| s.to_string());
        let tray = self.tray.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            self.lease.clone(),
            crate::lease::ToolKind::Mutate,
            mode,
            session_id.clone(),
            "clipboard_write",
            v,
            async move { tools::clipboard::clipboard_write(self, args).await },
        )
        .await)
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_audited<F, T>(
    audit: Arc<AuditLog>,
    tray: Arc<crate::tray::TrayState>,
    lease: Arc<crate::lease::LeaseStore>,
    tool_kind: crate::lease::ToolKind,
    mode: &'static str,
    session_id: Option<String>,
    tool: &'static str,
    args: serde_json::Value,
    fut: F,
) -> CallToolResult
where
    F: std::future::Future<Output = Result<T, ToolError>>,
    T: serde::Serialize,
{
    let lease_held = match tool_kind {
        crate::lease::ToolKind::Mutate => match lease.acquire(tool).await {
            Ok(()) => true,
            Err(e) => {
                let err: ToolError = e.into();
                let code = err.code();
                let msg = err.to_string();
                let details = serde_json::to_value(&err).unwrap_or(serde_json::Value::Null);
                let payload = serde_json::json!({ "code": code, "details": details });
                let r = CallToolResult::error(vec![
                    Content::text(msg),
                    Content::text(payload.to_string()),
                ]);
                let rec = crate::audit::AuditRecord {
                    ts: String::new(),
                    mode,
                    tool: tool.to_string(),
                    args: crate::audit::redact(tool, args),
                    ok: false,
                    duration_ms: Some(0),
                    error: Some(payload),
                    vision_fallback: None,
                    session_id: session_id.clone(),
                };
                if let Err(e) = audit.write(rec).await {
                    tracing::error!(error = %e, tool, "audit write failed");
                }
                return r;
            }
        },
        crate::lease::ToolKind::Observe => {
            // Best-effort refresh; ignore errors (lease file missing is fine).
            let _ = lease.refresh_if_holder();
            false
        }
    };
    let _guard = crate::tray::TrayGuard::enter(&tray, tool);
    let started = std::time::Instant::now();
    let outcome = fut.await;
    let duration_ms = started.elapsed().as_millis() as u64;

    // On cancel, release immediately so the next session can take over.
    if lease_held
        && (tray.cancel.is_cancelled() || matches!(outcome, Err(ToolError::Cancelled { .. })))
    {
        let _ = lease.release_if_holder();
    }

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
        session_id,
    };
    if let Err(e) = audit.write(rec).await {
        tracing::error!(error = %e, tool, "audit write failed");
    }
    call_result
}

/// Like [`dispatch_audited`], but the tool future yields `Vec<Content>`
/// directly instead of a serialisable typed payload. Used by tools that
/// need to return MCP image/audio content (e.g. `screenshot`) so the
/// payload reaches the model via its native vision/audio pathway
/// instead of being base64-encoded into a JSON-text content block.
///
/// Lease, tray-guard, timing, audit-record, and error-handling semantics
/// are identical to `dispatch_audited`. Keep the two in sync.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_audited_with_content<F>(
    audit: Arc<AuditLog>,
    tray: Arc<crate::tray::TrayState>,
    lease: Arc<crate::lease::LeaseStore>,
    tool_kind: crate::lease::ToolKind,
    mode: &'static str,
    session_id: Option<String>,
    tool: &'static str,
    args: serde_json::Value,
    fut: F,
) -> CallToolResult
where
    F: std::future::Future<Output = Result<Vec<Content>, ToolError>>,
{
    let lease_held = match tool_kind {
        crate::lease::ToolKind::Mutate => match lease.acquire(tool).await {
            Ok(()) => true,
            Err(e) => {
                let err: ToolError = e.into();
                let code = err.code();
                let msg = err.to_string();
                let details = serde_json::to_value(&err).unwrap_or(serde_json::Value::Null);
                let payload = serde_json::json!({ "code": code, "details": details });
                let r = CallToolResult::error(vec![
                    Content::text(msg),
                    Content::text(payload.to_string()),
                ]);
                let rec = crate::audit::AuditRecord {
                    ts: String::new(),
                    mode,
                    tool: tool.to_string(),
                    args: crate::audit::redact(tool, args),
                    ok: false,
                    duration_ms: Some(0),
                    error: Some(payload),
                    vision_fallback: None,
                    session_id: session_id.clone(),
                };
                if let Err(e) = audit.write(rec).await {
                    tracing::error!(error = %e, tool, "audit write failed");
                }
                return r;
            }
        },
        crate::lease::ToolKind::Observe => {
            let _ = lease.refresh_if_holder();
            false
        }
    };
    let _guard = crate::tray::TrayGuard::enter(&tray, tool);
    let started = std::time::Instant::now();
    let outcome = fut.await;
    let duration_ms = started.elapsed().as_millis() as u64;

    if lease_held
        && (tray.cancel.is_cancelled() || matches!(outcome, Err(ToolError::Cancelled { .. })))
    {
        let _ = lease.release_if_holder();
    }

    let redacted_args = crate::audit::redact(tool, args);
    let (call_result, audit_error) = match outcome {
        Ok(content) => (CallToolResult::success(content), None),
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
        session_id,
    };
    if let Err(e) = audit.write(rec).await {
        tracing::error!(error = %e, tool, "audit write failed");
    }
    call_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialise tests that mutate process-wide env vars (XDG_RUNTIME_DIR
    /// for the lease's tempdir setup).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    async fn fresh_audit() -> (tempfile::TempDir, Arc<AuditLog>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let audit = Arc::new(AuditLog::open(&path).await.unwrap());
        (dir, audit)
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_success() {
        let _env = ENV_LOCK.lock().unwrap();
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let xdg = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", xdg.path()) };
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel);
        let lease = std::sync::Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            "test".into(),
        ));
        let r = dispatch_audited(
            audit.clone(),
            tray,
            lease.clone(),
            crate::lease::ToolKind::Observe,
            "live",
            None,
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

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_error() {
        let _env = ENV_LOCK.lock().unwrap();
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let xdg = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", xdg.path()) };
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel);
        let lease = std::sync::Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            "test".into(),
        ));
        let r = dispatch_audited(
            audit.clone(),
            tray,
            lease.clone(),
            crate::lease::ToolKind::Mutate,
            "live",
            None,
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

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn dispatch_audited_busy_path_writes_audit_and_returns_error() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let (_audit_dir, audit) = fresh_audit().await;

        // Pre-write a lease record pointing at PID 1 (init, always alive),
        // so the dispatch_audited Mutate path will time out and return Busy.
        let lease_file = crate::lease::lease_path();
        std::fs::create_dir_all(lease_file.parent().unwrap()).unwrap();
        let blocker = crate::lease::LeaseRecord {
            holder_pid: 1,
            holder_session_id: "alien".into(),
            holder_intent: "keyboard.type".into(),
            acquired_at: chrono::Utc::now(),
            last_refresh_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        };
        std::fs::write(&lease_file, serde_json::to_string(&blocker).unwrap()).unwrap();

        // Build a LeaseStore for "us" (different PID — u32::MAX) with a
        // short acquire_timeout so the test runs quickly.
        let cfg = crate::lease::LeaseConfig {
            idle_timeout: std::time::Duration::from_secs(10),
            acquire_timeout: std::time::Duration::from_millis(250),
        };
        let lease = std::sync::Arc::new(crate::lease::LeaseStore::new(cfg, u32::MAX, "us".into()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel);
        let path = _audit_dir.path().join("a.jsonl");
        let r = dispatch_audited(
            audit.clone(),
            tray,
            lease,
            crate::lease::ToolKind::Mutate,
            "live",
            None,
            "keyboard.type",
            serde_json::json!({"text": "irrelevant"}),
            async {
                // This future should NOT run — the lease should reject
                // before we get here.
                panic!("future ran despite Busy");
                #[allow(unreachable_code)]
                Ok::<_, ToolError>(serde_json::Value::Null)
            },
        )
        .await;
        assert_eq!(r.is_error, Some(true));
        // Drop the audit log so the write is flushed.
        drop(audit);
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["tool"], "keyboard.type");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], "busy");
    }
}
