//! Daemon OAuth 2.1 transport tests (ADR-0014).
//!
//! These spin up a daemon with OAuth wiring on an ephemeral TCP port,
//! then exercise the wire surface: discovery metadata, token issuance,
//! the 401 challenge envelope, and a stale-token rejection.
//!
//! Live AF_UNIX behaviour is not exercised here (Unix transport has no
//! OAuth — fs perms cover it); see `daemon_http.rs` for the bare-rmcp
//! happy path without auth.

use kde_mcp::transport::oauth::OAuthState;
use kde_mcp::{KdeMcp, audit, cli::Mode};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

async fn test_server() -> (TempDir, KdeMcp) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let audit = Arc::new(audit::AuditLog::open(&path).await.expect("audit open"));
    let bus = zbus::Connection::session()
        .await
        .expect("test requires a session bus");
    (tmp, KdeMcp::new(Mode::Live, audit, bus))
}

/// Spawn an OAuth-enabled daemon on an ephemeral port; return the port,
/// the issuer URL the daemon advertises, and the in-test OAuth state
/// (so tests can pre-seed tokens).
async fn spawn_daemon(server: KdeMcp) -> (u16, String, OAuthState) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let port = addr.port();
    let issuer = format!("http://{addr}");
    let oauth = OAuthState::new(issuer.clone());
    let oauth_for_daemon = oauth.clone();
    tokio::spawn(async move {
        let _ = kde_mcp::transport::daemon::serve_with_listener_oauth(
            server,
            listener,
            oauth_for_daemon,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (port, issuer, oauth)
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthenticated_mcp_returns_proper_oauth_challenge() {
    let (_tmp, server) = test_server().await;
    let (port, issuer, _) = spawn_daemon(server).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 401);
    let www = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .expect("WWW-Authenticate header present");
    assert!(www.starts_with("Bearer "), "scheme: {www}");
    assert!(www.contains(r#"realm="kde-mcp""#), "realm missing: {www}");
    assert!(
        www.contains("resource_metadata="),
        "resource_metadata URL missing: {www}"
    );
    assert!(
        www.contains(&issuer),
        "metadata URL doesn't point at our issuer ({issuer}): {www}"
    );
    let body: serde_json::Value = resp.json().await.expect("json body");
    // RFC 6750 §3 envelope — Claude Code 2.1.139's OAuth client trips
    // on empty bodies (Unexpected EOF). We must return JSON.
    assert!(body.get("error").is_some(), "missing 'error': {body}");
    assert!(
        body.get("error_description").is_some(),
        "missing 'error_description': {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn well_known_protected_resource_metadata() {
    let (_tmp, server) = test_server().await;
    let (port, issuer, _) = spawn_daemon(server).await;
    let body: serde_json::Value = reqwest::get(format!(
        "http://127.0.0.1:{port}/.well-known/oauth-protected-resource"
    ))
    .await
    .expect("GET")
    .json()
    .await
    .expect("json");
    assert_eq!(body["resource"], format!("{issuer}/mcp"));
    let auths = body["authorization_servers"]
        .as_array()
        .expect("authorization_servers array");
    assert!(auths.iter().any(|v| v.as_str() == Some(&issuer)));
    let methods = body["bearer_methods_supported"]
        .as_array()
        .expect("bearer_methods_supported array");
    assert!(methods.iter().any(|v| v.as_str() == Some("header")));
}

#[tokio::test(flavor = "multi_thread")]
async fn well_known_authorization_server_metadata() {
    let (_tmp, server) = test_server().await;
    let (port, issuer, _) = spawn_daemon(server).await;
    let body: serde_json::Value = reqwest::get(format!(
        "http://127.0.0.1:{port}/.well-known/oauth-authorization-server"
    ))
    .await
    .expect("GET")
    .json()
    .await
    .expect("json");
    assert_eq!(body["issuer"], issuer);
    assert_eq!(
        body["token_endpoint"],
        format!("{issuer}/oauth/token"),
        "token_endpoint URL mismatch"
    );
    assert_eq!(
        body["registration_endpoint"],
        format!("{issuer}/oauth/register"),
    );
    let grants = body["grant_types_supported"]
        .as_array()
        .expect("grant_types_supported array");
    assert!(
        grants
            .iter()
            .any(|v| v.as_str() == Some("client_credentials"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn token_endpoint_issues_bearer_and_unlocks_mcp() {
    let (_tmp, server) = test_server().await;
    let (port, _, _) = spawn_daemon(server).await;
    let client = reqwest::Client::new();

    // Issue.
    let token_body: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await
        .expect("POST /oauth/token")
        .json()
        .await
        .expect("json");
    let access_token = token_body["access_token"]
        .as_str()
        .expect("access_token string")
        .to_string();
    assert_eq!(access_token.len(), 64, "32 bytes hex = 64 chars");
    assert_eq!(token_body["token_type"], "Bearer");
    assert!(
        token_body["expires_in"].as_u64().unwrap_or(0) >= 60,
        "expires_in should be a positive duration: {token_body}"
    );

    // Use against /mcp — should pass auth and reach rmcp.
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }
        }))
        .send()
        .await
        .expect("POST /mcp");
    // 200 means the bearer was accepted; anything else (401, 403)
    // means the OAuth wiring failed.
    assert_eq!(resp.status(), 200, "MCP rejected the issued bearer");
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_or_unknown_bearer_is_rejected_with_invalid_token() {
    let (_tmp, server) = test_server().await;
    let (port, _, _) = spawn_daemon(server).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Authorization", "Bearer not-a-real-token")
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(
        body["error"], "invalid_token",
        "expected invalid_token, got {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registration_endpoint_returns_opaque_client_id() {
    let (_tmp, server) = test_server().await;
    let (port, _, _) = spawn_daemon(server).await;
    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/oauth/register"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("POST /oauth/register");
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.expect("json");
    let id = body["client_id"].as_str().expect("client_id string");
    assert!(!id.is_empty(), "client_id must be non-empty");
    // RFC 7591: registration response must echo the auth method we
    // advertise. Ours is `none`.
    assert_eq!(body["token_endpoint_auth_method"], "none");
}
