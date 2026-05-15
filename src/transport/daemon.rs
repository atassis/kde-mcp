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
use crate::transport::session::{SessionRegistry, SessionScope};
use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, routing::get};
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

/// Shared state passed to the `/healthz` handler.
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
    // Watchdog signal: stamped on daemon startup and on every
    // `SessionScope::drop` (i.e. genuine session end). The idle
    // watchdog measures `last_active_at.elapsed() >= idle` only when
    // `registry.count() == 0`.
    let last_active_at = Arc::new(Mutex::new(Instant::now()));

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
    let tray_for_factory = server.tray.clone();
    let last_active_for_factory = last_active_at.clone();
    let mcp_service = StreamableHttpService::new(
        move || {
            let (session_id, _cancel) = registry_for_factory.open();
            sessions_for_factory.fetch_add(1, Ordering::Relaxed);
            tray_for_factory
                .sessions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::debug!(%session_id, "daemon: new mcp session");
            let audit_clone = audit_for_factory.clone();
            tokio::spawn(async move {
                let _ = audit_clone
                    .write_event(
                        "session_open",
                        json!({"session_id": session_id.to_string()}),
                    )
                    .await;
            });
            // The scope holds bookkeeping for session-end: when rmcp
            // drops the last clone of this per-session `KdeMcp`,
            // `SessionScope::drop` decrements the tray's session
            // counter, closes the registry entry, stamps
            // `last_active_at`, and writes `session_close`.
            let scope = Arc::new(SessionScope::new(
                session_id,
                registry_for_factory.clone(),
                tray_for_factory.clone(),
                audit_for_factory.clone(),
                last_active_for_factory.clone(),
            ));
            Ok(server_for_factory.with_session_scope(scope))
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

    // Idle watchdog — when the registry reports zero open MCP sessions
    // AND `last_active_at` (updated by `SessionScope::drop`) is past
    // the idle threshold, fire the stop token. `last_active_at` is
    // initialised to `Instant::now()` at startup so a fresh daemon
    // with no traffic idles out after `idle` wall-clock too.
    let stop_for_idle = stop.clone();
    let registry_for_idle = registry.clone();
    let last_active_for_idle = last_active_at.clone();
    let audit_for_idle = audit.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if stop_for_idle.is_cancelled() {
                return;
            }
            if registry_for_idle.count() == 0 {
                let last = *last_active_for_idle.lock();
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
