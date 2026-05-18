//! Verifies that `dispatch_audited[_with_content]` populates the
//! `timings` field on every audit record per spec §3.1 of
//! `docs/specs/2026-05-19-evaluation-and-latency-design.md`.
//!
//! Mirrors `daemon_http.rs` for daemon bootstrap; additionally tails the
//! audit JSONL to deserialise the produced record.

use kde_mcp::{KdeMcp, audit, cli::Mode};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

async fn test_server_audit() -> (TempDir, PathBuf, KdeMcp) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let audit = Arc::new(audit::AuditLog::open(&path).await.expect("audit open"));
    let bus = zbus::Connection::session()
        .await
        .expect("test requires a session bus");
    let srv = KdeMcp::new(Mode::Live, audit, bus);
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

/// Read the first audit line that belongs to the given `tool` and parse
/// it into an `AuditRecord`. Panics with the full body if no matching
/// line is found (cheap debugging signal in CI).
fn read_audit_record_for_tool(path: &Path, tool: &str) -> audit::AuditRecord {
    let body = std::fs::read_to_string(path).expect("read audit");
    let needle = format!("\"tool\":\"{tool}\"");
    let line = body
        .lines()
        .find(|l| l.contains(&needle))
        .unwrap_or_else(|| panic!("no audit line for tool={tool}; full body:\n{body}"));
    serde_json::from_str(line).expect("audit json parse")
}

#[tokio::test(flavor = "multi_thread")]
async fn audit_record_carries_five_timings_after_a_tool_call() {
    let (_tmp, audit_path, server) = test_server_audit().await;
    let port = spawn_daemon_on_ephemeral_port(server).await;
    let client = reqwest::Client::new();

    // 1. initialize + capture session id.
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

    // 2. tools/call observe — routes through dispatch_audited and writes
    //    an audit record we can inspect for the timings field.
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

    // 3. Audit writes are async-flushed.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let record = read_audit_record_for_tool(&audit_path, "observe");
    let t = record
        .timings
        .expect("timings must be present on observe record");

    // The five fields must form a monotonic non-decreasing sequence.
    assert_eq!(
        t.t_received_ns, 0,
        "t_received_ns is the request-start anchor; should be 0, got {}",
        t.t_received_ns
    );
    assert!(
        t.t_policy_done_ns >= t.t_received_ns,
        "t_policy_done_ns < t_received_ns: {t:?}"
    );
    assert!(
        t.t_impl_start_ns >= t.t_policy_done_ns,
        "t_impl_start_ns < t_policy_done_ns: {t:?}"
    );
    assert!(
        t.t_impl_done_ns >= t.t_impl_start_ns,
        "t_impl_done_ns < t_impl_start_ns: {t:?}"
    );
    assert!(
        t.t_audit_written_ns >= t.t_impl_done_ns,
        "t_audit_written_ns < t_impl_done_ns: {t:?}"
    );
    // Sanity: the impl phase must have taken some non-zero time.
    assert!(
        t.t_impl_done_ns > 0,
        "expected non-zero impl duration: {t:?}"
    );
}
