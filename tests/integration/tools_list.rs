use kde_mcp::KdeMcp;
use kde_mcp::audit::AuditLog;
use kde_mcp::cli::Mode;
use std::collections::HashSet;
use std::sync::Arc;

/// Post-ADR-0013, the wire surface is 12 tools. `keyboard.{type,key}`,
/// `window_{query,control}`, and `clipboard_{read,write}` are the three
/// split-along-safety pairs; everything else is one wire name per logical
/// tool. ADR-0013 removed `bash`, `text_editor.{view,edit}`, and `wait`.
const EXPECTED_WIRE_NAMES: &[&str] = &[
    "observe",
    "inspect",
    "screenshot",
    "read_text",
    "act_on_element",
    "mouse",
    "keyboard.type",
    "keyboard.key",
    "window_query",
    "window_control",
    "clipboard_read",
    "clipboard_write",
];

/// Stub state. Per Task 2 of Phase 1, `KdeMcp::new` requires an `AuditLog`
/// and a `zbus::Connection`. The tool-router tests below don't touch either,
/// they only enumerate the registered tool metadata — so we use a tempfile
/// audit and the dev machine's session bus. The session-bus dependency is
/// load-bearing only in the sense that these tests now require a running
/// user session (true today; CI may need a dbus-run-session wrapper later).
async fn test_state() -> KdeMcp {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.jsonl");
    // Leak the tempdir so the file stays valid for the test's lifetime.
    let _ = Box::leak(Box::new(dir));
    let audit = Arc::new(AuditLog::open(&path).await.unwrap());
    let bus = zbus::Connection::session()
        .await
        .expect("test requires a session bus");
    KdeMcp::new(Mode::Live, audit, bus)
}

#[tokio::test]
async fn server_advertises_twelve_tools_with_expected_names() {
    let server = test_state().await;
    let names: HashSet<String> = server
        .tool_router
        .list_all()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();

    let expected: HashSet<String> = EXPECTED_WIRE_NAMES.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        names.len(),
        12,
        "expected 12 wire tool names, got {names:?}"
    );
    assert!(
        names == expected,
        "tool name set drift — missing: {:?}, extra: {:?}",
        expected.difference(&names).collect::<Vec<_>>(),
        names.difference(&expected).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn read_only_hint_is_set_on_observation_tools() {
    let server = test_state().await;
    let observation = [
        "observe",
        "inspect",
        "screenshot",
        "read_text",
        "window_query",
        "clipboard_read",
    ];
    for tool in server.tool_router.list_all() {
        if observation.contains(&tool.name.as_ref()) {
            let hint = tool
                .annotations
                .as_ref()
                .and_then(|a| a.read_only_hint)
                .unwrap_or(false);
            assert!(hint, "{} should have read_only_hint=true", tool.name);
        }
    }
}

#[tokio::test]
async fn destructive_hint_is_set_on_mutating_tools() {
    let server = test_state().await;
    let mutating = [
        "act_on_element",
        "mouse",
        "keyboard.type",
        "keyboard.key",
        "window_control",
        "clipboard_write",
    ];
    for tool in server.tool_router.list_all() {
        if mutating.contains(&tool.name.as_ref()) {
            let hint = tool
                .annotations
                .as_ref()
                .and_then(|a| a.destructive_hint)
                .unwrap_or(false);
            assert!(hint, "{} should have destructive_hint=true", tool.name);
        }
    }
}

/// After ADR-0013 removed `bash`, no remaining tool carries
/// `openWorldHint: true`. Every action this MCP performs has a well-defined
/// scope (a window, an element, the clipboard, the desktop).
#[tokio::test]
async fn no_tool_has_open_world_hint() {
    let server = test_state().await;
    for tool in server.tool_router.list_all() {
        let open = tool
            .annotations
            .as_ref()
            .and_then(|a| a.open_world_hint)
            .unwrap_or(false);
        assert!(!open, "{} should not have open_world_hint=true", tool.name);
    }
}
