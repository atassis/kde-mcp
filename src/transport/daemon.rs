//! Daemon MCP transport — singleton per user (ADR-0012, amended
//! 2026-05-16 to add Unix/TCP peer transports, ADR-0015 to drop the
//! OAuth layer the TCP transport briefly carried).
//!
//! Two listener types in production:
//!
//!   - **Unix socket** (default) — `$XDG_RUNTIME_DIR/kde-mcp/mcp.sock`,
//!     mode 0600. Filesystem permissions on the parent dir
//!     (`XDG_RUNTIME_DIR` is 0700 per-user) plus the socket mode mean
//!     only the owning UID can connect. Same-UID processes are inside
//!     the threat model already — they could drive libei directly
//!     without going through kde-mcp.
//!   - **TCP loopback** — `127.0.0.1:7575`. No application-layer auth
//!     (ADR-0015): the 127.0.0.1 bind is the trust boundary. Empirically
//!     verified that Claude Code 2.1.139 connects unauthenticated when
//!     the server doesn't 401 and doesn't advertise OAuth discovery.
//!     Different-UID and external attackers are blocked by the bind;
//!     same-UID processes are inside the threat model regardless of
//!     what auth scheme we layer on top. Note: clients that respect
//!     `HTTPS_PROXY` for loopback URLs (Claude Code's HTTP MCP
//!     client) will route the request through the proxy and never
//!     reach us — set `NO_PROXY=127.0.0.1,localhost,::1` in the
//!     client's env. Prefer `--transport=unix` on KDE-Plasma-Wayland
//!     once Claude Code supports `unix://` URLs.
//!
//! Both listener types pick up an fd from systemd via `LISTEN_FDS`
//! when present; fall back to direct bind for dev / non-systemd runs.
//! The router and lifecycle logic is identical — only the listener
//! type differs.

use crate::server::KdeMcp;
use crate::transport::session::{SessionRegistry, SessionScope};
use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, routing::get};
use parking_lot::Mutex;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, UnixListener};
use tokio_util::sync::CancellationToken;

/// Production idle timeout: stop the daemon after 10 minutes of zero
/// active connections. systemd's `kde-mcp.socket` remains bound, so the
/// next request transparently re-activates the service.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// Default unix-socket path. Resolves to `/run/user/<uid>/kde-mcp/mcp.sock`
/// in normal Plasma sessions. `$XDG_RUNTIME_DIR` is tmpfs-backed,
/// per-user, mode 0700 — the socket inherits that confinement.
pub fn default_socket_path() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(d).join("kde-mcp/mcp.sock");
    }
    PathBuf::from("/tmp/kde-mcp.sock")
}

/// Shared state passed to the `/healthz` handler.
#[derive(Clone)]
struct HealthState {
    started_at: Instant,
    sessions: Arc<AtomicU64>,
}

/// Public entry: Unix-domain socket listener.
///
/// Picks up `LISTEN_FDS[0]` if systemd handed us a Unix socket; falls
/// back to binding `default_socket_path()` directly. No application-
/// layer auth — filesystem permissions on `$XDG_RUNTIME_DIR/kde-mcp/`
/// (0700) and the socket itself (0600) are the trust boundary.
pub async fn serve_unix(server: KdeMcp) -> Result<()> {
    let listener = pick_unix_listener().await?;
    serve_with_listener_and_timeout(server, listener, IDLE_TIMEOUT).await
}

/// Public entry: TCP `127.0.0.1:7575` listener.
///
/// Picks up `LISTEN_FDS[0]` if systemd handed us a TCP socket; falls
/// back to direct bind. No application-layer auth (ADR-0015) — the
/// 127.0.0.1 bind is the trust boundary; same-UID is inside the
/// threat model regardless.
pub async fn serve_http(server: KdeMcp) -> Result<()> {
    let listener = pick_tcp_listener().await?;
    serve_with_listener_and_timeout(server, listener, IDLE_TIMEOUT).await
}

/// Convenience for tests with default idle timeout. The listener is
/// generic so test code can pass a `TcpListener` for tooling ergonomics.
pub async fn serve_with_listener<L>(server: KdeMcp, listener: L) -> Result<()>
where
    L: axum::serve::Listener + Send + 'static,
    L::Addr: std::fmt::Debug + Send,
{
    serve_with_listener_and_timeout(server, listener, IDLE_TIMEOUT).await
}

pub async fn serve_with_listener_and_timeout<L>(
    server: KdeMcp,
    listener: L,
    idle: Duration,
) -> Result<()>
where
    L: axum::serve::Listener + Send + 'static,
    L::Addr: std::fmt::Debug + Send,
{
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

    // --- shutdown token (created early so subsystems can subscribe) -------
    let stop = CancellationToken::new();

    let mcp_router = Router::new().route_service("/mcp", mcp_service);
    let app = healthz_router.merge(mcp_router);

    // --- shutdown wiring ----------------------------------------------------

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

/// Returns a `UnixListener`. Prefers systemd's inherited fd when
/// `LISTEN_FDS` is set; falls back to binding `default_socket_path()`.
async fn pick_unix_listener() -> Result<UnixListener> {
    let mut lf = listenfd::ListenFd::from_env();
    if let Some(std_listener) = lf
        .take_unix_listener(0)
        .context("listenfd::take_unix_listener")?
    {
        std_listener
            .set_nonblocking(true)
            .context("set_nonblocking on inherited fd")?;
        return UnixListener::from_std(std_listener).context("UnixListener::from_std");
    }
    let path = default_socket_path();
    bind_unix_socket(&path)
        .await
        .with_context(|| format!("bind unix socket at {}", path.display()))
}

/// Returns a `TcpListener` on `127.0.0.1:7575`. Prefers systemd's
/// inherited fd; falls back to direct bind. Used by the deprecated
/// HTTP transport.
async fn pick_tcp_listener() -> Result<TcpListener> {
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

/// Bind a fresh Unix-domain socket at `path`, creating the parent dir
/// if needed and chmod'ing the socket to 0600. Idempotent — unlinks
/// any stale socket file left over from a previous crash.
pub(crate) async fn bind_unix_socket(path: &Path) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    // Stale socket file from a previous run blocks the bind; unlink it.
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)
        .with_context(|| format!("UnixListener::bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))?;
    Ok(listener)
}
