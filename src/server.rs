//! The rmcp wire surface.
//!
//! Per Phase 0 Spike 0.2, the `#[tool_router]` proc-macro does NOT compose
//! across multiple `impl` blocks (it emits a duplicate `tool_router()`
//! constructor → E0592). The 16 wire-named tools therefore live in one
//! `#[tool_router] impl KdeMcp` block below. Comment dividers preserve the
//! four-category grouping the spec §6 catalogue uses.

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
    pub tool_router: ToolRouter<KdeMcp>,
}

impl KdeMcp {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            refs: Arc::new(RefStore::new(Duration::from_secs(60))),
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
        Ok(tool_result(tools::observe::observe(args).await))
    }

    #[tool(
        description = "Drill into the AT-SPI tree of a window or element. Returns a flattened element list.",
        annotations(read_only_hint = true)
    )]
    async fn inspect(
        &self,
        Parameters(args): Parameters<tools::inspect::InspectArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::inspect::inspect(args).await))
    }

    #[tool(
        description = "Capture pixels. Prefer observe/inspect for structural questions. Use a targeted region over 'full' for token efficiency.",
        annotations(read_only_hint = true)
    )]
    async fn screenshot(
        &self,
        Parameters(args): Parameters<tools::screenshot::ScreenshotArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::screenshot::screenshot(args).await))
    }

    #[tool(
        description = "OCR a region. Use when AT-SPI doesn't expose text (canvas, PDF, image). Output is always trust: external.",
        annotations(read_only_hint = true)
    )]
    async fn read_text(
        &self,
        Parameters(args): Parameters<tools::read_text::ReadTextArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::read_text::read_text(args).await))
    }

    #[tool(
        description = "Block until a structured condition (window/element appears, screen changes, etc.). Replaces arbitrary sleeps.",
        annotations(read_only_hint = true)
    )]
    async fn wait(
        &self,
        Parameters(args): Parameters<tools::wait::WaitArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::wait::wait(args).await))
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
        Ok(tool_result(
            tools::act_on_element::act_on_element(args).await,
        ))
    }

    #[tool(
        description = "Mouse click/move/scroll/drag at coordinates. Vision fallback when AT-SPI is empty.",
        annotations(destructive_hint = true)
    )]
    async fn mouse(
        &self,
        Parameters(args): Parameters<tools::mouse::MouseArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::mouse::mouse(args).await))
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
        Ok(tool_result(tools::keyboard::keyboard_type(args).await))
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
        Ok(tool_result(tools::keyboard::keyboard_key(args).await))
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
        Ok(tool_result(tools::window::window_query(args).await))
    }

    #[tool(
        description = "Focus/minimise/maximise/close a window by ref.",
        annotations(destructive_hint = true)
    )]
    async fn window_control(
        &self,
        Parameters(args): Parameters<tools::window::WindowControlArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::window::window_control(args).await))
    }

    #[tool(
        description = "Read the clipboard.",
        annotations(read_only_hint = true)
    )]
    async fn clipboard_read(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardReadArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::clipboard::clipboard_read(args).await))
    }

    #[tool(
        description = "Set the clipboard contents.",
        annotations(destructive_hint = true)
    )]
    async fn clipboard_write(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardWriteArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(tool_result(tools::clipboard::clipboard_write(args).await))
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
        Ok(tool_result(tools::bash::bash(args).await))
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
        Ok(tool_result(
            tools::text_editor::text_editor_view(args).await,
        ))
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
        Ok(tool_result(
            tools::text_editor::text_editor_edit(args).await,
        ))
    }
}

#[tool_handler]
impl ServerHandler for KdeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
        .with_instructions(
            "kde-mcp: KDE Plasma 6 / Wayland desktop control. \
             Phase 0 build — all tools return not_implemented."
                .to_string(),
        )
    }
}

/// Convert a tool's Result into a CallToolResult.
///
/// Successes serialise to text content carrying the structured payload.
/// Semantic failures produce CallToolResult::error with the structured
/// `{ code, details }` shape per spec §8 — the model sees a typed error
/// code rather than free-text.
fn tool_result<T: serde::Serialize>(r: Result<T, ToolError>) -> CallToolResult {
    match r {
        Ok(payload) => {
            let v = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
            CallToolResult::success(vec![Content::text(v.to_string())])
        }
        Err(e) => {
            let code = e.code();
            let msg = e.to_string();
            let details = serde_json::to_value(&e).unwrap_or(serde_json::Value::Null);
            let payload = serde_json::json!({ "code": code, "details": details });
            CallToolResult::error(vec![
                Content::text(msg),
                Content::text(payload.to_string()),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_carries_error_code_in_payload() {
        let r: Result<(), ToolError> = Err(ToolError::NotImplemented);
        let result = tool_result(r);
        assert_eq!(result.is_error, Some(true));
        // The structured payload is in the second content block.
        let payload_text = match &result.content[1].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        let v: serde_json::Value = serde_json::from_str(payload_text).unwrap();
        assert_eq!(v["code"], "not_implemented");
    }

    #[test]
    fn tool_result_success_path_is_not_error() {
        #[derive(serde::Serialize)]
        struct Ok {}
        let r: Result<Ok, ToolError> = Ok(Ok {});
        let result = tool_result(r);
        assert!(matches!(result.is_error, None | Some(false)));
    }
}
