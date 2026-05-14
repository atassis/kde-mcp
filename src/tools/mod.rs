// Per-tool free-function modules. Each `<name>.rs` exposes the Args/Response
// structs and an async function that the central `KdeMcp` struct in
// src/server.rs delegates to from its #[tool]-annotated methods.
//
// In Phase 0 every function returns Err(ToolError::NotImplemented); the
// Args/Response shapes are skeletal — just enough for schemars to advertise
// the tool over MCP. The full spec §6 shapes land in the phase that
// implements each tool.

pub mod act_on_element;
pub mod bash;
pub mod clipboard;
pub mod inspect;
pub mod keyboard;
pub mod mouse;
pub mod observe;
pub mod read_text;
pub mod screenshot;
pub mod text_editor;
pub mod wait;
pub mod window;
