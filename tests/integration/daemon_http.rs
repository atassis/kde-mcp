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

async fn test_server_audit() -> (TempDir, std::path::PathBuf, KdeMcp) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let srv = test_server_with_audit_path(&path).await;
    (tmp, path, srv)
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

async fn spawn_daemon_with_auth(server: KdeMcp, token: &str) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let token = token.to_string();
    tokio::spawn(async move {
        let _ = kde_mcp::transport::daemon::serve_with_listener_and_token(
            server,
            listener,
            Some(token),
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

fn init_body() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    })
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
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init_body())
        .send()
        .await
        .expect("POST /mcp");
    assert!(
        resp.status().is_success(),
        "status={}",
        resp.status(),
    );
    let text = resp.text().await.expect("text body");
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
    let _ = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init_body())
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

#[tokio::test(flavor = "multi_thread")]
async fn http_rejects_missing_auth() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_with_auth(server, "deadbeef").await;
    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init_body())
        .send()
        .await
        .expect("POST /mcp");
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_rejects_wrong_bearer() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_with_auth(server, "deadbeef").await;
    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer feedface")
        .json(&init_body())
        .send()
        .await
        .expect("POST /mcp");
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_accepts_correct_bearer() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_with_auth(server, "deadbeef").await;
    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer deadbeef")
        .json(&init_body())
        .send()
        .await
        .expect("POST /mcp");
    assert!(resp.status().is_success(), "status={}", resp.status());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_healthz_does_not_require_auth() {
    let (_tmp, server) = test_server().await;
    let port = spawn_daemon_with_auth(server, "deadbeef").await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/healthz"))
        .await
        .expect("GET /healthz");
    assert!(resp.status().is_success());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_tool_call_audit_contains_session_id() {
    let (_tmp, audit_path, server) = test_server_audit().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    let client = reqwest::Client::new();

    // 1. initialize and grab the mcp-session-id header so subsequent calls
    //    are routed to the same session by rmcp.
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init_body())
        .send()
        .await
        .expect("POST /mcp init");
    let mcp_sid = resp
        .headers()
        .get("mcp-session-id")
        .expect("mcp-session-id header on initialize")
        .clone();

    // 2. tools/call observe → goes through dispatch_audited, which writes a
    //    record with our session_id attribution.
    let _ = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .header("mcp-session-id", mcp_sid)
        .json(&serde_json::json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"observe","arguments":{"detail":"summary"}}
        }))
        .send()
        .await
        .expect("POST /mcp tools/call");

    // 3. Audit writes are async-flushed; give it a moment.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.expect("read audit");
    let observe_line = body
        .lines()
        .find(|l| l.contains("\"tool\":\"observe\""))
        .expect(&format!("audit must contain an observe entry; full body:\n{body}"));
    let entry: serde_json::Value = serde_json::from_str(observe_line).expect("audit json");
    assert!(
        entry["session_id"].as_str().is_some(),
        "expected session_id on observe audit entry: {entry}"
    );
    // UUID format check: 36 chars with hyphens.
    let sid = entry["session_id"].as_str().unwrap();
    assert_eq!(sid.len(), 36, "session_id should be a UUID: {sid}");
    assert_eq!(sid.matches('-').count(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn daemon_idles_down_after_short_timeout() {
    let (_tmp, server) = test_server().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // 250 ms idle so the test runs fast.
    let idle = Duration::from_millis(250);
    let handle = tokio::spawn(async move {
        kde_mcp::transport::daemon::serve_with_listener_and_timeout(server, listener, None, idle)
            .await
    });
    // One request keeps `last_active_at` recent.
    let _ = reqwest::get(format!("http://127.0.0.1:{port}/healthz"))
        .await
        .expect("/healthz");
    // Now wait > idle so the watchdog fires.
    let res = tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .expect("daemon must exit within 3s of idle-stop firing");
    assert!(res.is_ok(), "daemon task panicked: {res:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_audit_events_recorded() {
    let (_tmp, audit_path, server) = test_server_audit().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    // Trigger session_open by issuing initialize.
    let _ = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&init_body())
        .send()
        .await
        .expect("init");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.expect("audit");
    assert!(
        body.contains("\"tool\":\"daemon_start\""),
        "daemon_start event missing from audit; body:\n{body}"
    );
    assert!(
        body.contains("\"tool\":\"session_open\""),
        "session_open event missing from audit; body:\n{body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_two_clients_get_distinct_session_ids() {
    let (_tmp, audit_path, server) = test_server_audit().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;

    async fn one_client(port: u16, id: u32) {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "jsonrpc":"2.0","id":id,"method":"initialize",
                "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}
            }))
            .send()
            .await
            .expect("init");
        let mcp_sid = resp.headers().get("mcp-session-id").expect("sid").clone();
        let _ = client
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("mcp-session-id", mcp_sid)
            .json(&serde_json::json!({
                "jsonrpc":"2.0","id":id+100,"method":"tools/call",
                "params":{"name":"observe","arguments":{"detail":"summary"}}
            }))
            .send()
            .await
            .expect("call");
    }
    let (_, _) = tokio::join!(one_client(port, 1), one_client(port, 2));

    tokio::time::sleep(Duration::from_millis(250)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.expect("audit");
    let sids: std::collections::HashSet<String> = body
        .lines()
        .filter(|l| l.contains("\"tool\":\"observe\""))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["session_id"].as_str().map(String::from))
        .collect();
    assert_eq!(
        sids.len(),
        2,
        "expected 2 distinct session_ids across observe audits, got {sids:?}; body:\n{body}"
    );
}
