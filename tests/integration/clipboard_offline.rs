use kde_mcp::cli::Mode;
use kde_mcp::tools::clipboard::{ClipboardReadArgs, clipboard_read};
use kde_mcp::{KdeMcp, audit::AuditLog};
use std::sync::Arc;
use tempfile::tempdir;

async fn test_state() -> KdeMcp {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    let _ = Box::leak(Box::new(dir));
    let audit = Arc::new(AuditLog::open(&path).await.unwrap());
    let bus = zbus::Connection::session().await.expect("session bus");
    KdeMcp::new(Mode::Live, audit, bus)
}

/// Either the clipboard is empty (external command failure on `wl-paste
/// --list-types`), or the requested MIME is not advertised (target_not_found),
/// or `wl-paste` is missing entirely (external_command_failed). All three
/// outcomes are valid offline results — we just shouldn't panic and the error
/// code should be one of the known taxonomy entries.
#[tokio::test]
async fn unknown_mime_returns_target_not_found_or_external_command_failed() {
    let state = test_state().await;
    let r = clipboard_read(
        &state,
        ClipboardReadArgs {
            mime_type: Some("text/wholly-fake".into()),
            encoding: Some("utf8".into()),
        },
    )
    .await;
    let err = r.unwrap_err();
    let ok_codes = ["target_not_found", "external_command_failed"];
    assert!(
        ok_codes.contains(&err.code()),
        "got code {} (message: {})",
        err.code(),
        err
    );
}
