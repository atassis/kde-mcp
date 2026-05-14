use kde_mcp::KdeMcp;
use kde_mcp::audit::AuditLog;
use kde_mcp::cli::Mode;
use std::collections::HashSet;
use std::sync::Arc;

/// Spec §6 names 15 logical tools. On the wire we expose 16 names because
/// keyboard/text_editor each split into a .type/.key and .view/.edit pair
/// (window_query/control and clipboard_read/write also count as two
/// distinct wire names but they're "two logical tools" in the spec's table
/// — so the 15→16 delta comes from the keyboard and text_editor splits).
const EXPECTED_WIRE_NAMES: &[&str] = &[
    "observe",
    "inspect",
    "screenshot",
    "read_text",
    "wait",
    "act_on_element",
    "mouse",
    "keyboard.type",
    "keyboard.key",
    "window_query",
    "window_control",
    "clipboard_read",
    "clipboard_write",
    "bash",
    "text_editor.view",
    "text_editor.edit",
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
async fn server_advertises_sixteen_tools_with_expected_names() {
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
        16,
        "expected 16 wire tool names, got {names:?}"
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
        "wait",
        "window_query",
        "clipboard_read",
        "text_editor.view",
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
        "bash",
        "text_editor.edit",
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

#[tokio::test]
async fn open_world_hint_is_set_only_on_bash() {
    let server = test_state().await;
    for tool in server.tool_router.list_all() {
        let open = tool
            .annotations
            .as_ref()
            .and_then(|a| a.open_world_hint)
            .unwrap_or(false);
        if tool.name == "bash" {
            assert!(open, "bash should have open_world_hint=true");
        } else {
            assert!(!open, "{} should not have open_world_hint=true", tool.name);
        }
    }
}
