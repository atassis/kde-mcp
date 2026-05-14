use kde_mcp::cli::Mode;
use kde_mcp::tools::text_editor::{TextEditorViewArgs, text_editor_view};
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

#[tokio::test]
async fn view_file_returns_numbered_lines() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("note.txt");
    tokio::fs::write(&path, "alpha\nbeta\ngamma\n")
        .await
        .unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(resp.kind, "file");
    let body = resp.content.unwrap();
    assert!(body.contains("1\talpha"));
    assert!(body.contains("2\tbeta"));
    assert!(body.contains("3\tgamma"));
    assert_eq!(resp.total_lines, Some(3));
}

#[tokio::test]
async fn view_file_respects_view_range_one_indexed_inclusive() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("multi.txt");
    let body = (1..=10)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(&path, body).await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: Some([3, 5]),
        },
    )
    .await
    .unwrap();
    let body = resp.content.unwrap();
    assert!(body.contains("3\tline3"));
    assert!(body.contains("5\tline5"));
    assert!(!body.contains("6\tline6"));
    assert!(!body.contains("2\tline2"));
}

#[tokio::test]
async fn view_with_view_range_end_eq_minus_one_means_eof() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("eof.txt");
    let body = (1..=5)
        .map(|i| format!("ln{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(&path, body).await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: Some([2, -1]),
        },
    )
    .await
    .unwrap();
    let body = resp.content.unwrap();
    assert!(body.contains("5\tln5"));
    assert!(!body.contains("1\tln1"));
}

#[tokio::test]
async fn view_directory_lists_entries_at_depth_2() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("sub");
    tokio::fs::create_dir(&nested).await.unwrap();
    tokio::fs::write(dir.path().join("top.txt"), "x")
        .await
        .unwrap();
    tokio::fs::write(nested.join("inner.txt"), "y")
        .await
        .unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: dir.path().to_string_lossy().to_string(),
            view_range: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(resp.kind, "directory");
    let entries = resp.entries.unwrap();
    let names: Vec<&str> = entries.iter().map(|s| s.as_str()).collect();
    assert!(
        names.iter().any(|n| n.ends_with("/top.txt")),
        "names: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.ends_with("/sub")),
        "names: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.ends_with("/sub/inner.txt")),
        "names: {names:?}"
    );
}

#[tokio::test]
async fn view_rejects_relative_path() {
    let state = test_state().await;
    let err = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: "relative/path".into(),
            view_range: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "permission_denied");
}

#[tokio::test]
async fn view_returns_io_error_on_missing_file() {
    let state = test_state().await;
    let err = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: "/this/does/not/exist.kdemcp.test".into(),
            view_range: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "io");
}
