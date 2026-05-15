//! Daemon (HTTP) MCP transport — singleton per user (ADR-0012).
//!
//! Phase 2: axum server with `/healthz`. Phase 3 mounts rmcp at `/mcp`.
//! Subsequent phases add auth, session attribution, idle-stop, tray
//! integration.
//!
//! The listening fd is preferentially picked up from systemd via
//! `LISTEN_FDS` (see `pick_listener`); falls back to binding
//! `127.0.0.1:7575` directly for dev / non-systemd runs.

use crate::server::KdeMcp;
use crate::transport::session::SessionRegistry;
use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, routing::get};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::net::TcpListener;

/// Shared state passed to the `/healthz` axum handler. `started_at`
/// powers `uptime_s`; `sessions` is bumped by the rmcp factory closure
/// each time a new client session opens, and decremented when it closes.
#[derive(Clone)]
struct HealthState {
    started_at: Instant,
    sessions: Arc<AtomicU64>,
}

pub async fn serve(server: KdeMcp) -> Result<()> {
    let listener = pick_listener().await?;
    let token_path = crate::transport::auth::default_token_path();
    let token = crate::transport::auth::ensure_token(&token_path)
        .with_context(|| format!("ensure_token at {}", token_path.display()))?;
    tracing::info!(token_path = %token_path.display(), "daemon: bearer token ready");
    serve_with_listener_and_token(server, listener, Some(token)).await
}

/// Convenience for tests: no auth, default token-less behaviour.
pub async fn serve_with_listener(server: KdeMcp, listener: TcpListener) -> Result<()> {
    serve_with_listener_and_token(server, listener, None).await
}

pub async fn serve_with_listener_and_token(
    server: KdeMcp,
    listener: TcpListener,
    token: Option<String>,
) -> Result<()> {
    use crate::transport::auth::{AuthState, bearer_middleware};

    let sessions = Arc::new(AtomicU64::new(0));
    let started_at = Instant::now();
    let registry = SessionRegistry::new();

    // Each new MCP session gets a fresh-cloned `KdeMcp` tagged with a
    // freshly-minted `SessionId`. The tag flows into the audit log via
    // `dispatch_audited` so every entry can be attributed to the
    // session that produced it.
    let server_for_factory = server.clone();
    let sessions_for_factory = sessions.clone();
    let registry_for_factory = registry.clone();
    let mcp_service = StreamableHttpService::new(
        move || {
            let (session_id, _cancel) = registry_for_factory.open();
            sessions_for_factory.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%session_id, "daemon: new mcp session");
            Ok(server_for_factory.with_session_id(session_id))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let health_state = HealthState {
        started_at,
        sessions,
    };

    let healthz_router = Router::new()
        .route("/healthz", get(healthz))
        .with_state(health_state);

    // /mcp goes through bearer auth (if a token is configured); /healthz
    // stays open. Keeping the two as separate sub-routers and merging
    // means the `.layer(...)` on the MCP sub-router doesn't accidentally
    // wrap /healthz.
    let mcp_router = Router::new().route_service("/mcp", mcp_service);
    let mcp_router = if let Some(tok) = token {
        let auth = AuthState { token: tok };
        mcp_router.layer(axum::middleware::from_fn_with_state(
            auth,
            bearer_middleware,
        ))
    } else {
        mcp_router
    };

    let app = healthz_router.merge(mcp_router);

    if let Ok(addr) = listener.local_addr() {
        tracing::info!(?addr, "daemon listening");
    }
    axum::serve(listener, app)
        .await
        .context("axum::serve")?;
    Ok(())
}

async fn healthz(State(state): State<HealthState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "uptime_s": state.started_at.elapsed().as_secs(),
        "sessions": state.sessions.load(Ordering::Relaxed),
    }))
}

/// Returns a listener — preferring systemd's inherited fd if available,
/// falling back to a fresh bind on `127.0.0.1:7575`. systemd sets
/// `LISTEN_FDS` and `LISTEN_PID` before exec; `listenfd` reads those and
/// hands us back the already-bound fd.
async fn pick_listener() -> Result<TcpListener> {
    let mut lf = listenfd::ListenFd::from_env();
    if let Some(std_listener) = lf
        .take_tcp_listener(0)
        .context("listenfd::take_tcp_listener")?
    {
        std_listener
            .set_nonblocking(true)
            .context("set_nonblocking on inherited fd")?;
        return TcpListener::from_std(std_listener).context("TcpListener::from_std");
    }
    TcpListener::bind("127.0.0.1:7575")
        .await
        .context("bind 127.0.0.1:7575")
}
