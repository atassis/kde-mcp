//! HTTP daemon transport tests (ADR-0012).
//!
//! These tests bring up the daemon on an ephemeral port and exercise it
//! via reqwest. They require a session bus (like all KdeMcp integration
//! tests). See `tests/integration/tools_list.rs` for the same pattern.

use kde_mcp::{KdeMcp, audit, cli::Mode};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

async fn test_server_with_audit_path(audit_path: &Path) -> KdeMcp {
    let audit = Arc::new(audit::AuditLog::open(audit_path).await.expect("audit open"));
    let bus = zbus::Connection::session()
        .await
        .expect("test requires a session bus");
    KdeMcp::new(Mode::Live, audit, bus)
}

async fn test_server() -> (TempDir, KdeMcp) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let srv = test_server_with_audit_path(&path).await;
    (tmp, srv)
}

async fn spawn_daemon_on_ephemeral_port(server: KdeMcp) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    tokio::spawn(async move {
        let _ = kde_mcp::transport::daemon::serve_with_listener(server, listener).await;
    });
    // Allow axum to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn healthz_reports_ok_without_auth() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    let body: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/healthz"))
        .await
        .expect("GET /healthz")
        .json()
        .await
        .expect("json");
    assert_eq!(body["ok"], true);
    assert!(body["uptime_s"].as_u64().is_some(), "uptime_s should be a u64: {body}");
    assert_eq!(body["sessions"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_initialize_returns_serverinfo() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    let client = reqwest::Client::new();
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init)
        .send()
        .await
        .expect("POST /mcp");
    assert!(
        resp.status().is_success(),
        "status={} body={:?}",
        resp.status(),
        resp.text().await
    );
    let text = resp.text().await.expect("text body");
    // rmcp's Streamable HTTP can reply either as JSON or as SSE-framed events.
    // Both forms carry `serverInfo` somewhere in the body; we just check for it.
    assert!(
        text.contains("\"name\":\"kde-mcp\"") || text.contains("\"name\": \"kde-mcp\""),
        "serverInfo.name missing from /mcp initialize response: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_initialize_bumps_session_counter() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    let client = reqwest::Client::new();
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    let _ = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init)
        .send()
        .await
        .expect("POST /mcp");
    let body: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/healthz"))
        .await
        .expect("GET /healthz")
        .json()
        .await
        .expect("json");
    assert_eq!(body["sessions"], 1, "expected sessions=1 after initialize: {body}");
}
