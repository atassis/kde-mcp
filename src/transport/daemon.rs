//! Daemon (HTTP) MCP transport — singleton per user (ADR-0012).
//!
//! Phases 2-6: axum server, `/healthz`, rmcp at `/mcp`, bearer auth,
//! per-session attribution, idle-stop watchdog, SIGTERM drain, lifecycle
//! audit events.
//!
//! The listening fd is preferentially picked up from systemd via
//! `LISTEN_FDS` (see `pick_listener`); falls back to binding
//! `127.0.0.1:7575` directly for dev / non-systemd runs.

use crate::server::KdeMcp;
use crate::transport::session::SessionRegistry;
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::Request,
    middleware::Next,
    response::Response,
    routing::get,
};
use parking_lot::Mutex;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Production idle timeout: stop the daemon after 10 minutes of zero
/// active connections. systemd's `kde-mcp.socket` remains bound, so the
/// next request transparently re-activates the service.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// Shared state passed to axum handlers.
#[derive(Clone)]
struct HealthState {
    started_at: Instant,
    sessions: Arc<AtomicU64>,
}

#[derive(Clone)]
struct ActivityState {
    active_conns: Arc<AtomicU64>,
    last_active_at: Arc<Mutex<Instant>>,
}

pub async fn serve(server: KdeMcp) -> Result<()> {
    let listener = pick_listener().await?;
    let token_path = crate::transport::auth::default_token_path();
    let token = crate::transport::auth::ensure_token(&token_path)
        .with_context(|| format!("ensure_token at {}", token_path.display()))?;
    tracing::info!(token_path = %token_path.display(), "daemon: bearer token ready");
    serve_with_listener_and_timeout(server, listener, Some(token), IDLE_TIMEOUT).await
}

/// Convenience for tests: no auth, default idle timeout.
pub async fn serve_with_listener(server: KdeMcp, listener: TcpListener) -> Result<()> {
    serve_with_listener_and_timeout(server, listener, None, IDLE_TIMEOUT).await
}

/// Convenience for tests: token-aware, default idle timeout.
pub async fn serve_with_listener_and_token(
    server: KdeMcp,
    listener: TcpListener,
    token: Option<String>,
) -> Result<()> {
    serve_with_listener_and_timeout(server, listener, token, IDLE_TIMEOUT).await
}

pub async fn serve_with_listener_and_timeout(
    server: KdeMcp,
    listener: TcpListener,
    token: Option<String>,
    idle: Duration,
) -> Result<()> {
    use crate::transport::auth::{AuthState, bearer_middleware};

    // --- shared state -------------------------------------------------------
    let sessions = Arc::new(AtomicU64::new(0));
    let started_at = Instant::now();
    let registry = SessionRegistry::new();
    let activity = ActivityState {
        active_conns: Arc::new(AtomicU64::new(0)),
        last_active_at: Arc::new(Mutex::new(Instant::now())),
    };

    let audit = server.audit.clone();
    audit
        .write_event("daemon_start", json!({}))
        .await
        .map_err(|e| anyhow::anyhow!("audit daemon_start: {e}"))?;

    // --- rmcp service factory ----------------------------------------------
    let server_for_factory = server.clone();
    let sessions_for_factory = sessions.clone();
    let registry_for_factory = registry.clone();
    let audit_for_factory = audit.clone();
    let mcp_service = StreamableHttpService::new(
        move || {
            let (session_id, _cancel) = registry_for_factory.open();
            sessions_for_factory.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%session_id, "daemon: new mcp session");
            let audit = audit_for_factory.clone();
            tokio::spawn(async move {
                let _ = audit
                    .write_event(
                        "session_open",
                        json!({"session_id": session_id.to_string()}),
                    )
                    .await;
            });
            Ok(server_for_factory.with_session_id(session_id))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    // --- routers ------------------------------------------------------------
    let health_state = HealthState {
        started_at,
        sessions: sessions.clone(),
    };
    let healthz_router = Router::new()
        .route("/healthz", get(healthz))
        .with_state(health_state);

    let mcp_router = Router::new()
        .route_service("/mcp", mcp_service)
        .layer(axum::middleware::from_fn_with_state(
            activity.clone(),
            conn_tracking_middleware,
        ));

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

    // --- shutdown wiring ----------------------------------------------------
    let stop = CancellationToken::new();

    // SIGTERM handler — cancels the stop token to start graceful drain.
    let stop_for_sigterm = stop.clone();
    let audit_for_sigterm = audit.clone();
    tokio::spawn(async move {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                tracing::info!("daemon: SIGTERM received, draining");
                let _ = audit_for_sigterm
                    .write_event("daemon_sigterm", json!({}))
                    .await;
                stop_for_sigterm.cancel();
            }
            Err(e) => {
                tracing::warn!("daemon: SIGTERM handler install failed: {e}");
            }
        }
    });

    // Idle watchdog — if no active connections for `idle`, fire the stop token.
    let stop_for_idle = stop.clone();
    let activity_for_idle = activity.clone();
    let audit_for_idle = audit.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if stop_for_idle.is_cancelled() {
                return;
            }
            if activity_for_idle
                .active_conns
                .load(Ordering::Relaxed)
                == 0
            {
                let last = *activity_for_idle.last_active_at.lock();
                if last.elapsed() >= idle {
                    tracing::info!(?idle, "daemon: idle stop");
                    let _ = audit_for_idle
                        .write_event("daemon_idle_stop", json!({}))
                        .await;
                    stop_for_idle.cancel();
                    return;
                }
            }
        }
    });

    // --- serve --------------------------------------------------------------
    if let Ok(addr) = listener.local_addr() {
        tracing::info!(?addr, "daemon listening");
    }

    let stop_for_axum = stop.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            stop_for_axum.cancelled().await;
        })
        .await
        .context("axum::serve")
}

async fn healthz(State(state): State<HealthState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "uptime_s": state.started_at.elapsed().as_secs(),
        "sessions": state.sessions.load(Ordering::Relaxed),
    }))
}

/// axum middleware that bumps `active_conns` on entry, refreshes
/// `last_active_at`, and decrements on response. The idle watchdog
/// inspects both atomics to decide when to fire its stop token.
async fn conn_tracking_middleware(
    State(state): State<ActivityState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    state.active_conns.fetch_add(1, Ordering::Relaxed);
    *state.last_active_at.lock() = Instant::now();
    let resp = next.run(req).await;
    state.active_conns.fetch_sub(1, Ordering::Relaxed);
    *state.last_active_at.lock() = Instant::now();
    resp
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
