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

/// Shared phase-timing state for both dispatchers.
///
/// Two dispatchers (`dispatch_audited`, `dispatch_audited_with_content`)
/// follow the same five-phase lifecycle — they only differ in the
/// `CallToolResult` payload shape (typed-serialise vs `Vec<Content>`).
/// `TimingState` captures the lifecycle so the two payload paths can
/// share one copy of the boundary captures.
struct TimingState {
    request_start: std::time::Instant,
    t_received_ns: u64,
    t_policy_done_ns: u64,
    t_impl_start_ns: u64,
    t_impl_done_ns: u64,
}

impl TimingState {
    fn start() -> Self {
        Self {
            request_start: std::time::Instant::now(),
            t_received_ns: 0,
            t_policy_done_ns: 0,
            t_impl_start_ns: 0,
            t_impl_done_ns: 0,
        }
    }

    /// Capture the policy-done / impl-start boundary (they collapse onto
    /// the same instant — no work happens between them in either path).
    fn mark_policy_impl_boundary(&mut self) {
        let ns = self.request_start.elapsed().as_nanos() as u64;
        self.t_policy_done_ns = ns;
        self.t_impl_start_ns = ns;
    }

    /// Capture impl-done immediately after the tool future returns.
    fn mark_impl_done(&mut self) {
        self.t_impl_done_ns = self.request_start.elapsed().as_nanos() as u64;
    }

    /// Finalize into an `AuditTimings`. Captures `t_audit_written_ns`
    /// just before this returns — caller must call this immediately
    /// before `audit.write(rec).await` so the timing field reflects
    /// the boundary the spec §3.1 intends.
    ///
    /// Spec §3.1 calls for "after flush", but the flush IS the
    /// measured operation — an exact-after capture would require a
    /// double-write. The within-record delta is < 1 µs typically; we
    /// accept that imprecision in exchange for a single-write audit path.
    fn finish(self) -> crate::audit::AuditTimings {
        let t_audit_written_ns = self.request_start.elapsed().as_nanos() as u64;
        crate::audit::AuditTimings {
            t_received_ns: self.t_received_ns,
            t_policy_done_ns: self.t_policy_done_ns,
            t_impl_start_ns: self.t_impl_start_ns,
            t_impl_done_ns: self.t_impl_done_ns,
            t_audit_written_ns,
        }
    }

    /// Lease-refused early-return: policy/impl collapse onto the
    /// instant of refusal, impl-done equals impl-start.
    fn collapse_for_lease_refused(&mut self) {
        let ns = self.request_start.elapsed().as_nanos() as u64;
        self.t_policy_done_ns = ns;
        self.t_impl_start_ns = ns;
        self.t_impl_done_ns = ns;
    }
}

/// Translate a `ToolError` into the JSON `error` payload both
/// dispatchers store in `AuditRecord.error` and surface in the
/// `CallToolResult` content. Shape is `{ "code": <slug>, "details": <serialized err> }`,
/// matching the inline construction this helper replaces.
fn build_error_payload(err: &ToolError) -> serde_json::Value {
    let code = err.code();
    let details = serde_json::to_value(err).unwrap_or(serde_json::Value::Null);
    serde_json::json!({ "code": code, "details": details })
}

/// Construct an `AuditRecord` and flush it through the log, logging
/// any flush failure. Centralises the four post-dispatch writes the
/// two dispatchers used to inline. `ts` is left empty here — the
/// audit layer populates it on write.
#[allow(clippy::too_many_arguments)]
async fn write_audit_record(
    audit: &Arc<AuditLog>,
    mode: &str,
    tool: &str,
    args: serde_json::Value,
    ok: bool,
    duration_ms: Option<u64>,
    error: Option<serde_json::Value>,
    session_id: Option<String>,
    timings: Option<crate::audit::AuditTimings>,
) {
    let rec = crate::audit::AuditRecord {
        ts: String::new(),
        mode: mode.to_string(),
        tool: tool.to_string(),
        args,
        ok,
        duration_ms,
        error,
        vision_fallback: None,
        session_id,
        // Spec §4.13 row 10: env-driven harness tag. Cheap to read on
        // every dispatch (a getenv syscall on Linux); if it later shows
        // up in latency profiles we can cache it at daemon startup.
        session_tag: crate::audit::read_session_tag(),
        timings,
    };
    if let Err(e) = audit.write(rec).await {
        tracing::error!(error = %e, tool, "audit write failed");
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
    // Five-phase nanosecond budget per spec §3.1
    // (docs/specs/2026-05-19-evaluation-and-latency-design.md). All deltas
    // are relative to `ts.request_start`, so t_received_ns is always 0.
    let mut ts = TimingState::start();
    let lease_held = match tool_kind {
        crate::lease::ToolKind::Mutate => match lease.acquire(tool).await {
            Ok(()) => true,
            Err(e) => {
                let err: ToolError = e.into();
                let msg = err.to_string();
                let payload = build_error_payload(&err);
                let r = CallToolResult::error(vec![
                    Content::text(msg),
                    Content::text(payload.to_string()),
                ]);
                // Lease-refused path: the "policy"/"impl" boundaries collapse
                // onto the same instant since we never entered the tool body.
                ts.collapse_for_lease_refused();
                write_audit_record(
                    &audit,
                    mode,
                    tool,
                    crate::audit::redact(tool, args),
                    false,
                    Some(0),
                    Some(payload),
                    session_id.clone(),
                    Some(ts.finish()),
                )
                .await;
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
    // Policy + lease + tray-guard setup done; about to enter the tool body.
    ts.mark_policy_impl_boundary();
    let started = std::time::Instant::now();
    let outcome = if lease_held {
        lease.run_mutation(fut).await
    } else {
        fut.await
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    ts.mark_impl_done();

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
            let msg = e.to_string();
            let payload = build_error_payload(&e);
            let r =
                CallToolResult::error(vec![Content::text(msg), Content::text(payload.to_string())]);
            (r, Some(payload))
        }
    };
    let ok = audit_error.is_none();
    write_audit_record(
        &audit,
        mode,
        tool,
        redacted_args,
        ok,
        Some(duration_ms),
        audit_error,
        session_id,
        Some(ts.finish()),
    )
    .await;
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
    // Mirror dispatch_audited's five-phase budget via the shared
    // `TimingState` helper (see the doc comment above this function).
    let mut ts = TimingState::start();
    let lease_held = match tool_kind {
        crate::lease::ToolKind::Mutate => match lease.acquire(tool).await {
            Ok(()) => true,
            Err(e) => {
                let err: ToolError = e.into();
                let msg = err.to_string();
                let payload = build_error_payload(&err);
                let r = CallToolResult::error(vec![
                    Content::text(msg),
                    Content::text(payload.to_string()),
                ]);
                // Lease-refused: policy/impl boundaries collapse onto the
                // same instant; we never entered the tool body.
                ts.collapse_for_lease_refused();
                write_audit_record(
                    &audit,
                    mode,
                    tool,
                    crate::audit::redact(tool, args),
                    false,
                    Some(0),
                    Some(payload),
                    session_id.clone(),
                    Some(ts.finish()),
                )
                .await;
                return r;
            }
        },
        crate::lease::ToolKind::Observe => {
            let _ = lease.refresh_if_holder();
            false
        }
    };
    let _guard = crate::tray::TrayGuard::enter(&tray, tool);
    ts.mark_policy_impl_boundary();
    let started = std::time::Instant::now();
    let outcome = if lease_held {
        lease.run_mutation(fut).await
    } else {
        fut.await
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    ts.mark_impl_done();

    if lease_held
        && (tray.cancel.is_cancelled() || matches!(outcome, Err(ToolError::Cancelled { .. })))
    {
        let _ = lease.release_if_holder();
    }

    let redacted_args = crate::audit::redact(tool, args);
    let (call_result, audit_error) = match outcome {
        Ok(content) => (CallToolResult::success(content), None),
        Err(e) => {
            let msg = e.to_string();
            let payload = build_error_payload(&e);
            let r =
                CallToolResult::error(vec![Content::text(msg), Content::text(payload.to_string())]);
            (r, Some(payload))
        }
    };
    let ok = audit_error.is_none();
    write_audit_record(
        &audit,
        mode,
        tool,
        redacted_args,
        ok,
        Some(duration_ms),
        audit_error,
        session_id,
        Some(ts.finish()),
    )
    .await;
    call_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::tests::ENV_LOCK;

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
    async fn both_dispatchers_emit_identically_shaped_timings() {
        // Run dispatch_audited with a trivial Ok payload and
        // dispatch_audited_with_content with a trivial Vec<Content>.
        // Read the resulting AuditRecords; assert timings.is_some() and that
        // t_received_ns <= t_policy_done_ns <= t_impl_start_ns
        // <= t_impl_done_ns <= t_audit_written_ns on both records.
        let _env = ENV_LOCK.lock().unwrap();
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let xdg = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", xdg.path()) };
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = crate::tray::TrayState::new(cancel.clone());
        let lease = std::sync::Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            "test".into(),
        ));

        // (1) typed-payload dispatcher
        let _ = dispatch_audited(
            audit.clone(),
            tray.clone(),
            lease.clone(),
            crate::lease::ToolKind::Observe,
            "live",
            None,
            "test_a",
            serde_json::Value::Null,
            async { Ok::<_, ToolError>(serde_json::json!({"ok": true})) },
        )
        .await;

        // (2) content-payload dispatcher
        let _ = dispatch_audited_with_content(
            audit.clone(),
            tray.clone(),
            lease.clone(),
            crate::lease::ToolKind::Observe,
            "live",
            None,
            "test_b",
            serde_json::Value::Null,
            async { Ok::<_, ToolError>(vec![Content::text("hello")]) },
        )
        .await;

        // Flush by dropping our reference; one strong reference remains in
        // dispatchers' captured args, but `audit.write` already completed
        // synchronously inside each dispatcher (it awaits the write).
        drop(audit);
        let records = read_two_records(&path).await;
        for rec in &records {
            let t = rec.timings.as_ref().expect("timings present");
            // t_received_ns is fixed at 0 today (see TimingState::start);
            // the first inequality is therefore held by construction. Kept
            // as documentation of the intended monotonicity invariant.
            assert!(t.t_policy_done_ns >= t.t_received_ns);
            assert!(t.t_impl_start_ns >= t.t_policy_done_ns);
            assert!(t.t_impl_done_ns >= t.t_impl_start_ns);
            assert!(t.t_audit_written_ns >= t.t_impl_done_ns);
        }
        assert_eq!(records[0].tool, "test_a");
        assert_eq!(records[1].tool, "test_b");
    }

    async fn read_two_records(path: &std::path::Path) -> Vec<crate::audit::AuditRecord> {
        for _ in 0..50 {
            let body = tokio::fs::read_to_string(path).await.unwrap_or_default();
            let lines: Vec<_> = body.lines().filter(|l| !l.is_empty()).collect();
            if lines.len() >= 2 {
                return lines
                    .iter()
                    .map(|l| serde_json::from_str::<crate::audit::AuditRecord>(l).unwrap())
                    .filter(|r| r.tool == "test_a" || r.tool == "test_b")
                    .collect();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("audit records not flushed within 1s");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn mutation_keeps_lease_alive_while_waiting_for_approval() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let (_audit_dir, audit) = fresh_audit().await;
        let lease = Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig {
                idle_timeout: Duration::from_millis(60),
                acquire_timeout: Duration::from_millis(100),
            },
            std::process::id(),
            "approval-test".into(),
        ));
        let tray = crate::tray::TrayState::new(tokio_util::sync::CancellationToken::new());
        let response = dispatch_audited(
            audit,
            tray,
            lease,
            crate::lease::ToolKind::Mutate,
            "live",
            None,
            "window_control",
            serde_json::json!({}),
            async {
                tokio::time::sleep(Duration::from_millis(160)).await;
                let record: crate::lease::LeaseRecord = serde_json::from_str(
                    &std::fs::read_to_string(crate::lease::lease_path()).unwrap(),
                )
                .unwrap();
                assert!(
                    record.expires_at > chrono::Utc::now(),
                    "approval wait let the lease expire"
                );
                Ok::<_, ToolError>(serde_json::json!({"ok": true}))
            },
        )
        .await;
        assert_ne!(response.is_error, Some(true));
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn lost_mutation_lease_stops_pending_action() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let lease = crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig {
                idle_timeout: Duration::from_millis(60),
                acquire_timeout: Duration::from_millis(100),
            },
            std::process::id(),
            "approval-test".into(),
        );
        lease.acquire("window_control").await.unwrap();
        let mut executed = false;
        let result = lease
            .run_mutation(async {
                // Simulate losing ownership while blocked for an answer.
                std::fs::remove_file(crate::lease::lease_path()).unwrap();
                tokio::time::sleep(Duration::from_millis(120)).await;
                executed = true;
                Ok::<_, ToolError>(())
            })
            .await;
        assert!(matches!(result, Err(ToolError::PermissionDenied { .. })));
        assert!(!executed);
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
