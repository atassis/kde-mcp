# Singleton Daemon Implementation Plan

> Historical planning artifact — captures the plan as written on the date in the filename; the shipped implementation is the source of truth (see the ADRs and the design spec).

**Goal:** Add a long-running daemon mode that serves MCP over HTTP on `127.0.0.1:7575`, replacing the per-Claude-Code-session subprocess explosion (one tray icon, one AT-SPI subscription, one KWin script connection per user instead of one per session). Stdio mode is preserved as a co-equal peer for headless/CI. Implements `docs/specs/2026-05-15-singleton-daemon-design.md` + ADR-0012.

**Architecture:** One binary, two transports. New `--transport=stdio|daemon` CLI arg (defaulting to `stdio`) chooses between the existing stdio path and a new `axum`-based HTTP server using rmcp's `transport-streamable-http-server`. Daemon mode runs under systemd-user socket activation; the listening fd is passed in by systemd via `LISTEN_FDS`. Bearer-token auth from `$XDG_RUNTIME_DIR/kde-mcp/token`. Per-HTTP-connection session_id (UUID v4) threads through the existing ADR-0011 lease (no schema change), a new in-memory `DashMap<SessionId, CancellationToken>`, and the audit log (new optional `session_id` field).

**Tech Stack:** `axum 0.8` (HTTP server), `listenfd 1.0` (systemd socket-activation fd pickup), `uuid 1.10` (session IDs), `dashmap 6.1` (concurrent registries), `rand 0.9` (token gen), `rmcp 1.7` (already present, enable `transport-streamable-http-server` feature). Two new systemd unit files under `packaging/`. No changes to `tokio` features or to `zbus`.

**Spec note (one deviation):** The spec uses `--mode=stdio|daemon`, but the existing CLI's `--mode=live|virtual` already occupies that flag (desktop targeting — orthogonal to wire transport). This plan introduces `--transport=stdio|daemon` instead. Task 1.1 amends spec §3 to use the new name.

---

## Spec → Task map

| Spec section | Implementing task(s) |
|---|---|
| §3 Two co-equal modes | T1.1, T1.2 |
| §4 Daemon lifecycle (units, fd, idle, shutdown) | T2.1–T2.3, T6.1–T6.3 |
| §5 HTTP server (`axum`, `/mcp`, `/healthz`) | T2.2–T2.3, T3.1–T3.2 |
| §6 Authentication (bearer token, file perms) | T4.1–T4.3 |
| §7 Per-session attribution (id mint, partitioning) | T5.1–T5.4 |
| §8 Code layout impact | structured throughout |
| §9 Audit log (`session_id` field, new event types) | T5.3, T6.4 |
| §10 Migration & install | T8.1–T8.3 |
| §11 Fixes / costs (covered by tests) | T2.4, T3.3, T5.5, T6.5 |
| §12 Alternatives (no impl work) | n/a |
| §13 Implementation phasing | this whole plan |

---

## File map

**New files:**
- `src/transport/mod.rs` — `Transport` enum + dispatch.
- `src/transport/stdio.rs` — extracted from current `main.rs` body.
- `src/transport/daemon.rs` — axum router, idle loop, drain-on-SIGTERM.
- `src/transport/auth.rs` — bearer-token middleware + token file management.
- `src/transport/session.rs` — `SessionId`, `SessionRegistry` (`DashMap<SessionId, SessionState>`).
- `packaging/kde-mcp.socket` — systemd socket unit.
- `packaging/kde-mcp.service` — systemd service unit.
- `tests/integration/daemon_http.rs` — multi-session HTTP integration test.

**Modified files:**
- `Cargo.toml` — add `axum`, `listenfd`, `uuid`, `dashmap`, `rand`; add `transport-streamable-http-server` feature to `rmcp`.
- `src/cli.rs` — add `Transport` enum + `--transport` arg.
- `src/main.rs` — slim to: parse args → dispatch to transport.
- `src/lib.rs` — add `pub mod transport;`.
- `src/audit.rs` — add optional `session_id: Option<String>` field to `AuditRecord`.
- `src/server.rs` — accept an optional `SessionId` through `dispatch_audited`; populate the audit field.
- `src/cancel.rs` — split into per-session token registry + shared "abort all" token (daemon only).
- `src/tray.rs` — daemon-mode tooltip cycles "Idle / N sessions: <tool>"; add "Abort current session" menu item.
- `packaging/install.sh` — `--transport=stdio|daemon|both` flag; daemon flow installs units + writes Claude Code HTTP config with the bearer token.
- `docs/specs/2026-05-15-singleton-daemon-design.md` — rename `--mode=` to `--transport=` in §3 table.
- `docs/runbook/AUTONOMOUS-VERIFICATION.md` — add Layer-0 transport detection.
- `docs/runbook/KNOWN-FIXES.md` — add F10 (`Authorization` header missing) and F11 (daemon socket not bound).

---

# Phase 1 — Plumbing (CLI arg + transport dispatch stub)

Release-safe no-op: adds the `--transport` flag, defaults to `stdio`, daemon path is a stub that exits with "not yet implemented." The runbook smoke test continues to pass unchanged.

## Task 1.1: Add `Transport` enum + `--transport` CLI arg

**Files:**
- Modify: `src/cli.rs`
- Test: `tests/integration/cli.rs` (or wherever existing CLI tests live)
- Modify: `docs/specs/2026-05-15-singleton-daemon-design.md` (rename `--mode=stdio|daemon` → `--transport=stdio|daemon` in §3)

- [ ] **Step 1: Write the failing test for the new CLI arg**

Append to `tests/integration/cli.rs` (create if missing — minimal harness using `kde_mcp::cli::parse_and_validate`):

```rust
#[test]
fn transport_defaults_to_stdio() {
    let cli =
        kde_mcp::cli::parse_and_validate(["kde-mcp", "--mode=live"].into_iter().map(String::from))
            .expect("parse");
    assert_eq!(cli.transport, kde_mcp::cli::Transport::Stdio);
}

#[test]
fn transport_daemon_accepted() {
    let cli = kde_mcp::cli::parse_and_validate(
        ["kde-mcp", "--mode=live", "--transport=daemon"]
            .into_iter()
            .map(String::from),
    )
    .expect("parse");
    assert_eq!(cli.transport, kde_mcp::cli::Transport::Daemon);
}

#[test]
fn transport_rejects_unknown() {
    let err = kde_mcp::cli::parse_and_validate(
        ["kde-mcp", "--mode=live", "--transport=quux"]
            .into_iter()
            .map(String::from),
    )
    .unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
}
```

- [ ] **Step 2: Run to verify failure**

```
cargo test --workspace transport_
```

Expected: 3 compile errors — `Transport` enum, `cli.transport` field do not exist yet.

- [ ] **Step 3: Implement the enum + arg in `src/cli.rs`**

After the `Mode` enum (around line 8), add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "lowercase")]
pub enum Transport {
    /// MCP over stdin/stdout. Default. One client per process; the
    /// process exits on stdin EOF. Used by Claude Code's `--transport
    /// stdio` and by the runbook smoke test.
    #[default]
    Stdio,
    /// MCP over HTTP on `127.0.0.1:7575`. Long-running; serves any
    /// number of concurrent connections. Activated by systemd-user
    /// socket activation. See ADR-0012.
    Daemon,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Daemon => "daemon",
        }
    }
}
```

Add a field to `Cli` (after `vnc`):

```rust
/// Wire transport. `stdio` (default) is one-client-per-process over
/// stdin/stdout; `daemon` is a long-running HTTP server on
/// `127.0.0.1:7575` activated by systemd-user. See ADR-0012.
#[arg(long, value_enum, default_value_t = Transport::Stdio)]
pub transport: Transport,
```

- [ ] **Step 4: Run tests to verify pass**

```
cargo test --workspace transport_
```

Expected: 3 tests pass.

- [ ] **Step 5: Amend the spec**

In `docs/specs/2026-05-15-singleton-daemon-design.md` §3 table, change `--mode=stdio` and `--mode=daemon` in the row labels to `--transport=stdio` and `--transport=daemon`. Add one sentence under the table:

> The flag is `--transport`, not `--mode`, because the existing `--mode=live|virtual` already names a different (orthogonal) axis: desktop targeting.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs tests/integration/cli.rs docs/specs/2026-05-15-singleton-daemon-design.md
git commit -m "feat(cli): add --transport=stdio|daemon (phase 1, ADR-0012)"
```

## Task 1.2: Dispatch in `main.rs` via a new `transport` module

**Files:**
- Create: `src/transport/mod.rs`
- Create: `src/transport/stdio.rs`
- Create: `src/transport/daemon.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing test for daemon-stub behaviour**

Append to `tests/integration/cli.rs`:

```rust
#[test]
fn transport_module_is_public() {
    // Spot-check: the dispatch fns referenced from main.rs are reachable
    // from the library. Stops us from making them private by accident.
    let _ = kde_mcp::transport::Transport::Stdio;
    let _ = kde_mcp::transport::Transport::Daemon;
}
```

- [ ] **Step 2: Run to verify failure**

```
cargo test --workspace transport_module_is_public
```

Expected: compile error — `kde_mcp::transport` does not exist.

- [ ] **Step 3: Create `src/transport/mod.rs`**

```rust
//! Wire-transport dispatch (ADR-0012). `Transport::Stdio` keeps the
//! one-client-per-process semantics from before this ADR; `Daemon` is
//! the new long-running HTTP server activated by systemd-user.
//!
//! Re-exports `Transport` so call sites don't depend on the CLI module.

pub use crate::cli::Transport;

pub mod daemon;
pub mod stdio;
```

- [ ] **Step 4: Create `src/transport/stdio.rs`**

Move the post-bootstrap logic from `main.rs` here:

```rust
use crate::server::KdeMcp;
use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;

pub async fn serve(server: Arc<KdeMcp>) -> Result<()> {
    // Borrowed-clone of lease for shutdown release; the underlying state
    // is in the Arc.
    let lease = server.lease.clone();
    let mode_name = server.mode_name();

    let service = (*server)
        .clone()
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("rmcp serve: {e}"))?;

    service.waiting().await?;

    lease.release_if_holder().ok();
    crate::notify::notify_lease_released(mode_name).await;
    tracing::info!("shut down cleanly");
    Ok(())
}
```

Note: `KdeMcp::clone()` must produce a working clone (likely already wrapped in `Arc` internally — verify in step 6). If `KdeMcp` is not `Clone`, this phase introduces a wrapping `Arc<KdeMcp>` at construction time and dereferences when calling `serve`.

- [ ] **Step 5: Create `src/transport/daemon.rs` stub**

```rust
use crate::server::KdeMcp;
use anyhow::{Result, anyhow};
use std::sync::Arc;

pub async fn serve(_server: Arc<KdeMcp>) -> Result<()> {
    Err(anyhow!(
        "daemon mode is not yet implemented (see plan docs/plans/2026-05-16-singleton-daemon-implementation.md phase 2+)"
    ))
}
```

- [ ] **Step 6: Update `src/lib.rs`**

Add after the `pub mod tray;` line:

```rust
pub mod transport;
```

- [ ] **Step 7: Refactor `src/main.rs` to dispatch**

Replace the body after `eprintln!("kde-mcp: ready ...")` with:

```rust
let server = Arc::new(server);

match cli.transport {
    kde_mcp::cli::Transport::Stdio => kde_mcp::transport::stdio::serve(server).await,
    kde_mcp::cli::Transport::Daemon => kde_mcp::transport::daemon::serve(server).await,
}
```

The `eprintln!` should now include the transport too:

```rust
eprintln!(
    "kde-mcp: ready (mode={mode_name}, transport={})",
    cli.transport.as_str()
);
```

- [ ] **Step 8: If `KdeMcp` is not `Clone`, add a `mode_name()` accessor**

Append to the `impl KdeMcp` block in `src/server.rs`:

```rust
pub fn mode_name(&self) -> &'static str {
    self.mode.as_str()
}
```

(Skip this step if `KdeMcp.mode` is already public-readable.)

- [ ] **Step 9: Run all tests**

```
cargo test --workspace 2>&1 | grep '^test result'
```

Expected: all suites pass, count is ≥ existing count.

- [ ] **Step 10: Smoke test in stdio mode**

```bash
./packaging/install.sh --skip-mcp
~/.local/bin/kde-mcp --mode=live --transport=stdio < /dev/null &
sleep 1
kill %1 2>/dev/null
```

Expected: process boots, prints `kde-mcp: ready (mode=live, transport=stdio)`, exits cleanly on stdin close.

- [ ] **Step 11: Smoke test daemon stub**

```bash
~/.local/bin/kde-mcp --mode=live --transport=daemon
```

Expected: prints the "daemon mode is not yet implemented" error and exits non-zero. Stderr contains the plan path.

- [ ] **Step 12: Commit**

```bash
git add src/transport/ src/main.rs src/lib.rs src/server.rs tests/integration/cli.rs
git commit -m "feat(transport): split into stdio/daemon dispatch (phase 1, ADR-0012)"
```

---

# Phase 2 — HTTP scaffold (axum + listenfd + /healthz)

End of phase: a daemon that listens on `127.0.0.1:7575`, responds to `GET /healthz` with `{"ok":true,"uptime_s":N,"sessions":0}`, and can be socket-activated by systemd.

## Task 2.1: Add HTTP-server dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Edit `Cargo.toml`**

In `[dependencies]`, add (alphabetical placement; pick existing nearby crate as anchor):

```toml
axum           = "0.8"
dashmap        = "6.1"
listenfd       = "1.0"
rand           = "0.9"
uuid           = { version = "1.10", features = ["v4", "serde"] }
```

In the existing `rmcp` line, add the `transport-streamable-http-server` feature:

```toml
rmcp           = { version = "1.7", features = ["server", "macros", "transport-io", "transport-streamable-http-server"] }
```

- [ ] **Step 2: Build to confirm deps resolve**

```
cargo build --release 2>&1 | tail -5
```

Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add axum/listenfd/uuid/dashmap/rand + rmcp HTTP feature (phase 2, ADR-0012)"
```

## Task 2.2: Build the `/healthz` axum server

**Files:**
- Modify: `src/transport/daemon.rs`
- Test: `tests/integration/daemon_http.rs` (new)

- [ ] **Step 1: Write the failing test**

Create `tests/integration/daemon_http.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn healthz_reports_ok_without_auth() {
    // Bring up the daemon on an ephemeral port; client connects and
    // GETs /healthz; assert shape.
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon(server, "127.0.0.1:0").await;
    let body = reqwest::get(format!("http://127.0.0.1:{port}/healthz"))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(body["ok"], true);
    assert!(body["uptime_s"].as_u64().is_some());
    assert_eq!(body["sessions"], 0);
}

mod test_support {
    use super::*;

    pub async fn test_server() -> Arc<kde_mcp::KdeMcp> {
        // Construct a KdeMcp wired for tests (re-use the helper used by
        // existing integration tests; if none exists, build one here that
        // points at /tmp paths for audit + lease).
        todo!("re-use existing test KdeMcp builder; see tests/integration/lease.rs")
    }

    pub async fn spawn_daemon(server: Arc<kde_mcp::KdeMcp>, addr: &str) -> u16 {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            kde_mcp::transport::daemon::serve_with_listener(server, listener)
                .await
                .ok();
        });
        // Give the server a moment to start accepting.
        tokio::time::sleep(Duration::from_millis(50)).await;
        port
    }
}
```

Add a dev-dependency:

```toml
[dev-dependencies]
reqwest        = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
```

(Skip if already present in dev-deps.)

- [ ] **Step 2: Run test to confirm failure**

```
cargo test --test daemon_http healthz_reports_ok_without_auth
```

Expected: compile error — `serve_with_listener` doesn't exist; `test_server` is `todo!()`.

- [ ] **Step 3: Implement the daemon's `/healthz` route**

Replace `src/transport/daemon.rs`:

```rust
use crate::server::KdeMcp;
use anyhow::Result;
use axum::{Json, Router, extract::State, routing::get};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

#[derive(Clone)]
struct DaemonState {
    started_at: Instant,
    sessions: Arc<dashmap::DashMap<uuid::Uuid, ()>>,
    server: Arc<KdeMcp>,
}

pub async fn serve(server: Arc<KdeMcp>) -> Result<()> {
    let listener = pick_listener().await?;
    serve_with_listener(server, listener).await
}

pub async fn serve_with_listener(server: Arc<KdeMcp>, listener: TcpListener) -> Result<()> {
    let state = DaemonState {
        started_at: Instant::now(),
        sessions: Arc::new(dashmap::DashMap::new()),
        server,
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .with_state(state);
    tracing::info!(addr = ?listener.local_addr().ok(), "daemon listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz(State(state): State<DaemonState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "uptime_s": state.started_at.elapsed().as_secs(),
        "sessions": state.sessions.len(),
    }))
}

async fn pick_listener() -> Result<TcpListener> {
    // systemd socket activation: LISTEN_FDS>=1 means fd 3 is our listener.
    let mut lf = listenfd::ListenFd::from_env();
    if let Some(std_listener) = lf.take_tcp_listener(0)? {
        std_listener.set_nonblocking(true)?;
        return Ok(TcpListener::from_std(std_listener)?);
    }
    // Fallback for dev / non-systemd runs.
    Ok(TcpListener::bind("127.0.0.1:7575").await?)
}
```

- [ ] **Step 4: Provide the `test_server` helper**

Find the existing test helper that constructs an in-memory `KdeMcp` (look in `tests/integration/lease.rs` or `tests/integration/server.rs`). Re-export it (or copy it) into `tests/integration/daemon_http.rs`'s `test_support` mod. Replace the `todo!()`. If no helper exists yet, build a minimal one:

```rust
pub async fn test_server() -> Arc<kde_mcp::KdeMcp> {
    use kde_mcp::{KdeMcp, audit, cli::Mode};
    let bus = zbus::Connection::session().await.unwrap();
    let audit = Arc::new(
        audit::AuditLog::open(&std::env::temp_dir().join("kde-mcp-test-audit.jsonl"))
            .await
            .unwrap(),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    Arc::new(KdeMcp::new_with_cancel(Mode::Live, audit, bus, cancel))
}
```

- [ ] **Step 5: Run test to confirm pass**

```
cargo test --test daemon_http healthz_reports_ok_without_auth -- --nocapture
```

Expected: PASS. `uptime_s` is a small integer; `sessions` is 0.

- [ ] **Step 6: Commit**

```bash
git add src/transport/daemon.rs tests/integration/daemon_http.rs Cargo.toml Cargo.lock
git commit -m "feat(daemon): axum scaffold + /healthz route (phase 2, ADR-0012)"
```

## Task 2.3: systemd socket-activation unit files

**Files:**
- Create: `packaging/kde-mcp.socket`
- Create: `packaging/kde-mcp.service`

- [ ] **Step 1: Write the socket unit**

Create `packaging/kde-mcp.socket`:

```ini
[Unit]
Description=kde-mcp daemon socket
PartOf=kde-mcp.service

[Socket]
ListenStream=127.0.0.1:7575
NoDelay=true
Accept=false

[Install]
WantedBy=sockets.target
```

- [ ] **Step 2: Write the service unit**

Create `packaging/kde-mcp.service`:

```ini
[Unit]
Description=kde-mcp daemon (HTTP MCP server)
Requires=kde-mcp.socket
After=kde-mcp.socket graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/kde-mcp --mode=live --transport=daemon
Restart=on-failure
RestartSec=2
TimeoutStopSec=5

[Install]
WantedBy=default.target
```

- [ ] **Step 3: Manual smoke test (one-time)**

```bash
install -D -m 0644 packaging/kde-mcp.socket  ~/.config/systemd/user/kde-mcp.socket
install -D -m 0644 packaging/kde-mcp.service ~/.config/systemd/user/kde-mcp.service
systemctl --user daemon-reload
systemctl --user enable --now kde-mcp.socket
ss -tlnp | grep 7575        # expected: LISTEN on 127.0.0.1:7575
curl -s http://127.0.0.1:7575/healthz    # expected: {"ok":true,"uptime_s":..,"sessions":0}
journalctl --user -u kde-mcp.service --since "1 minute ago" | tail -10
```

Expected: socket bound, service spawns on first `curl`, healthz returns the JSON.

- [ ] **Step 4: Commit**

```bash
git add packaging/kde-mcp.socket packaging/kde-mcp.service
git commit -m "feat(packaging): systemd-user socket-activation units (phase 2, ADR-0012)"
```

---

# Phase 3 — rmcp Streamable HTTP transport mounted

End of phase: `POST /mcp` is a working MCP endpoint backed by the same `dispatch_audited` the stdio mode uses. One client per connection works end-to-end.

## Task 3.1: Mount the rmcp HTTP transport

**Files:**
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `tests/integration/daemon_http.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn http_initialize_returns_serverinfo() {
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon(server, "127.0.0.1:0").await;
    let client = reqwest::Client::new();
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "status={}", resp.status());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    let server_info = &body["result"]["serverInfo"];
    assert_eq!(server_info["name"].as_str().unwrap(), "kde-mcp");
}
```

- [ ] **Step 2: Run to verify failure**

```
cargo test --test daemon_http http_initialize -- --nocapture
```

Expected: 404 from `/mcp` (route missing) or the test fails parsing.

- [ ] **Step 3: Mount rmcp at `/mcp`**

In `src/transport/daemon.rs`, replace `serve_with_listener` body:

```rust
pub async fn serve_with_listener(server: Arc<KdeMcp>, listener: TcpListener) -> Result<()> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpService,
        session::local::LocalSessionManager,
    };
    let state = DaemonState {
        started_at: Instant::now(),
        sessions: Arc::new(dashmap::DashMap::new()),
        server: server.clone(),
    };
    let mcp_service = StreamableHttpService::new(
        move || Ok((*server).clone()),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let app = Router::new()
        .route("/healthz", get(healthz))
        .nest_service("/mcp", mcp_service)
        .with_state(state);
    tracing::info!(addr = ?listener.local_addr().ok(), "daemon listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

The exact rmcp API surface (`StreamableHttpService::new`, `LocalSessionManager`, the closure that constructs the service for each session) is the canonical one from rmcp 1.7 examples; if signature changes by 1.8, adjust per the in-tree examples at `~/.cargo/registry/src/index.crates.io-*/rmcp-1.7.0/examples/`.

`KdeMcp` must be `Clone`. If it isn't, wrap each field that needs interior mutability in `Arc` (or `Arc<Mutex<...>>`) and derive `Clone`; this is consistent with the post-Phase 1 shape of the struct.

- [ ] **Step 4: Run test to verify pass**

```
cargo test --test daemon_http http_initialize -- --nocapture
```

Expected: PASS. Response body contains `serverInfo.name == "kde-mcp"`.

- [ ] **Step 5: Cross-check with `claude mcp` once locally**

```bash
claude mcp remove kde-mcp-http 2>/dev/null
claude mcp add --transport http kde-mcp-http http://127.0.0.1:7575/mcp
claude mcp list | grep kde-mcp-http
```

Expected: `✓ Connected`.

- [ ] **Step 6: Commit**

```bash
git add src/transport/daemon.rs tests/integration/daemon_http.rs
git commit -m "feat(daemon): mount rmcp streamable HTTP at /mcp (phase 3, ADR-0012)"
```

## Task 3.2: One tool call over HTTP

**Files:**
- Test: `tests/integration/daemon_http.rs`

- [ ] **Step 1: Add a tool-call test**

Append to the same file:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn http_call_observe_summary() {
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon(server, "127.0.0.1:0").await;
    let client = reqwest::Client::new();

    // Step 1: initialize.
    let init = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}});
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&init).send().await.unwrap();
    let session_header = resp
        .headers()
        .get("mcp-session-id")
        .cloned()
        .expect("mcp-session-id header on initialize response");

    // Step 2: tools/call observe
    let call = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"observe","arguments":{"detail":"summary"}}});
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_header)
        .json(&call).send().await.unwrap();
    assert!(resp.status().is_success(), "status={}", resp.status());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["result"].is_object() || body["error"].is_object(),
        "expected result or error: {body}");
}
```

- [ ] **Step 2: Run test**

```
cargo test --test daemon_http http_call_observe_summary -- --nocapture
```

Expected: PASS. (The test accepts either `result` or `error`: if the host has no graphical session under test conditions, `observe` may surface an error; the test asserts the wire layer works, not the live KWin behaviour.)

- [ ] **Step 3: Commit**

```bash
git add tests/integration/daemon_http.rs
git commit -m "test(daemon): one tool/call observe end-to-end over HTTP (phase 3)"
```

---

# Phase 4 — Bearer-token auth

End of phase: `POST /mcp` requires `Authorization: Bearer <token>`. Token is at `$XDG_RUNTIME_DIR/kde-mcp/token` (mode 0600), generated on first daemon launch.

## Task 4.1: Token generation + load

**Files:**
- Create: `src/transport/auth.rs`
- Modify: `src/transport/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/integration/auth.rs`:

```rust
use std::path::PathBuf;

#[test]
fn ensure_token_creates_file_with_0600() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("token");
    let tok = kde_mcp::transport::auth::ensure_token(&path).expect("write");
    assert_eq!(tok.len(), 64, "32 bytes = 64 hex chars");
    let meta = std::fs::metadata(&path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
}

#[test]
fn ensure_token_returns_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("token");
    let first = kde_mcp::transport::auth::ensure_token(&path).expect("create");
    let second = kde_mcp::transport::auth::ensure_token(&path).expect("read");
    assert_eq!(first, second, "second call must return the stored token, not mint a new one");
}
```

Add `tempfile` to `[dev-dependencies]` in `Cargo.toml` if not present.

- [ ] **Step 2: Run to confirm failure**

```
cargo test --test auth ensure_token
```

Expected: `kde_mcp::transport::auth::ensure_token` does not exist.

- [ ] **Step 3: Implement `src/transport/auth.rs`**

```rust
use anyhow::{Context, Result};
use rand::RngCore;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Path of the bearer-token file. `$XDG_RUNTIME_DIR/kde-mcp/token`, with
/// `/run/user/<uid>/kde-mcp/token` as the typical concrete path on Plasma.
pub fn default_token_path() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(d).join("kde-mcp/token");
    }
    PathBuf::from("/tmp/kde-mcp-token")
}

/// Read the token at `path`. If the file is missing, generate a new
/// 32-byte hex token, write it with mode 0600, then return it.
pub fn ensure_token(path: &Path) -> Result<String> {
    if let Ok(existing) = fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::write(path, &hex).with_context(|| format!("write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))?;
    Ok(hex)
}
```

- [ ] **Step 4: Re-export from `src/transport/mod.rs`**

```rust
pub mod auth;
```

- [ ] **Step 5: Run tests**

```
cargo test --test auth ensure_token
```

Expected: both pass.

- [ ] **Step 6: Commit**

```bash
git add src/transport/auth.rs src/transport/mod.rs tests/integration/auth.rs Cargo.toml Cargo.lock
git commit -m "feat(daemon): bearer-token file generation in $XDG_RUNTIME_DIR/kde-mcp/token (phase 4)"
```

## Task 4.2: axum middleware enforcing `Authorization: Bearer <token>` on `/mcp`

**Files:**
- Modify: `src/transport/auth.rs`
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests/integration/daemon_http.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn http_rejects_missing_auth() {
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "deadbeef").await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
        .send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_accepts_correct_bearer() {
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "deadbeef").await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer deadbeef")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
        .send().await.unwrap();
    assert!(resp.status().is_success(), "status={}", resp.status());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_healthz_does_not_require_auth() {
    let server = test_support::test_server().await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "deadbeef").await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await.unwrap();
    assert!(resp.status().is_success());
}
```

Extend `test_support`:

```rust
pub async fn spawn_daemon_with_auth(server: Arc<kde_mcp::KdeMcp>, addr: &str, token: &str) -> u16 {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = token.to_string();
    tokio::spawn(async move {
        kde_mcp::transport::daemon::serve_with_listener_and_token(server, listener, Some(token))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test --test daemon_http http_rejects_missing_auth http_accepts_correct_bearer http_healthz_does_not_require_auth -- --nocapture
```

Expected: at least one compile error (`serve_with_listener_and_token` missing) and/or 200 from `/mcp` without auth (no enforcement yet).

- [ ] **Step 3: Implement the middleware in `src/transport/auth.rs`**

Append:

```rust
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

#[derive(Clone)]
pub struct AuthState {
    pub token: String,
}

pub async fn bearer_middleware(
    State(state): State<AuthState>,
    headers: HeaderMap,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let want = format!("Bearer {}", state.token);
    let got = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if got == want {
        Ok(next.run(req).await)
    } else {
        tracing::warn!("daemon: 401 (Authorization {})", if got.is_empty() { "missing" } else { "wrong" });
        Err(StatusCode::UNAUTHORIZED)
    }
}
```

- [ ] **Step 4: Wire middleware into daemon, with token plumbed in**

In `src/transport/daemon.rs`, change `serve_with_listener` to take a token option, and route `/mcp` through the middleware:

```rust
pub async fn serve_with_listener(server: Arc<KdeMcp>, listener: TcpListener) -> Result<()> {
    let token_path = crate::transport::auth::default_token_path();
    let token = crate::transport::auth::ensure_token(&token_path)
        .context("ensure_token")?;
    serve_with_listener_and_token(server, listener, Some(token)).await
}

pub async fn serve_with_listener_and_token(
    server: Arc<KdeMcp>,
    listener: TcpListener,
    token: Option<String>,
) -> Result<()> {
    use crate::transport::auth::{AuthState, bearer_middleware};
    use rmcp::transport::streamable_http_server::{
        StreamableHttpService,
        session::local::LocalSessionManager,
    };
    let state = DaemonState {
        started_at: Instant::now(),
        sessions: Arc::new(dashmap::DashMap::new()),
        server: server.clone(),
    };
    let mcp_service = StreamableHttpService::new(
        move || Ok((*server).clone()),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let mut mcp_router = Router::new().nest_service("/", mcp_service);
    if let Some(tok) = token {
        let auth = AuthState { token: tok };
        mcp_router = mcp_router.layer(axum::middleware::from_fn_with_state(auth.clone(), bearer_middleware))
            .with_state(auth);
    }
    let app = Router::new()
        .route("/healthz", get(healthz))
        .nest("/mcp", mcp_router)
        .with_state(state);
    tracing::info!(addr = ?listener.local_addr().ok(), "daemon listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 5: Run tests**

```
cargo test --test daemon_http -- --nocapture
```

Expected: all three new tests pass; previously-passing tests (initialize, observe) continue to pass with `Authorization` header (you'll need to update them too — add `.header("Authorization", "Bearer deadbeef")` to the calls inside `http_initialize_returns_serverinfo` and `http_call_observe_summary`).

- [ ] **Step 6: Commit**

```bash
git add src/transport/auth.rs src/transport/daemon.rs tests/integration/daemon_http.rs
git commit -m "feat(daemon): bearer-token middleware on /mcp; /healthz stays open (phase 4)"
```

---

# Phase 5 — Per-session attribution

End of phase: each HTTP connection has a `session_id` (UUID v4). The audit log carries it. The lease's existing `holder_session_id` is populated from it. A `DashMap<SessionId, CancellationToken>` registry exists for the cancel-token split landing in phase 7.

## Task 5.1: `SessionId` newtype + registry

**Files:**
- Create: `src/transport/session.rs`
- Modify: `src/transport/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/integration/session.rs`:

```rust
use kde_mcp::transport::session::{SessionId, SessionRegistry};

#[test]
fn registry_mints_unique_ids() {
    let r = SessionRegistry::new();
    let (id_a, _) = r.open();
    let (id_b, _) = r.open();
    assert_ne!(id_a, id_b);
    assert_eq!(r.count(), 2);
    r.close(id_a);
    assert_eq!(r.count(), 1);
}

#[test]
fn session_id_round_trips_via_string() {
    let id = SessionId::new();
    let s = id.to_string();
    let back: SessionId = s.parse().expect("parse");
    assert_eq!(id, back);
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test --test session
```

Expected: compile error — `transport::session` missing.

- [ ] **Step 3: Implement `src/transport/session.rs`**

```rust
use dashmap::DashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        SessionId(Uuid::new_v4())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SessionId(Uuid::parse_str(s)?))
    }
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub cancel: CancellationToken,
}

#[derive(Default, Clone)]
pub struct SessionRegistry {
    inner: Arc<DashMap<SessionId, SessionEntry>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh session_id and its cancel-token; return both.
    pub fn open(&self) -> (SessionId, CancellationToken) {
        let id = SessionId::new();
        let cancel = CancellationToken::new();
        self.inner.insert(id, SessionEntry { cancel: cancel.clone() });
        (id, cancel)
    }

    pub fn close(&self, id: SessionId) {
        self.inner.remove(&id);
    }

    pub fn count(&self) -> usize {
        self.inner.len()
    }

    pub fn cancel(&self, id: SessionId) -> bool {
        self.inner.get(&id).map(|e| { e.cancel.cancel(); true }).unwrap_or(false)
    }

    pub fn cancel_all(&self) {
        for entry in self.inner.iter() {
            entry.cancel.cancel();
        }
    }
}
```

- [ ] **Step 4: Re-export from `src/transport/mod.rs`**

```rust
pub mod session;
```

- [ ] **Step 5: Run tests**

```
cargo test --test session
```

Expected: both pass.

- [ ] **Step 6: Commit**

```bash
git add src/transport/session.rs src/transport/mod.rs tests/integration/session.rs
git commit -m "feat(daemon): SessionId + SessionRegistry (phase 5)"
```

## Task 5.2: Add `session_id` to `AuditRecord`

**Files:**
- Modify: `src/audit.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/audit.rs`'s `#[cfg(test)] mod tests` (or its existing `writes_a_parseable_jsonl_line`):

```rust
#[tokio::test]
async fn audit_includes_session_id_when_set() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("audit.jsonl");
    let log = AuditLog::open(&path).await.unwrap();
    log.write(AuditRecord {
        ts: "2026-01-01T00:00:00Z".into(),
        mode: "live",
        tool: "observe".into(),
        args: serde_json::json!({}),
        ok: true,
        duration_ms: Some(5),
        error: None,
        vision_fallback: None,
        session_id: Some("11111111-2222-3333-4444-555555555555".into()),
    })
    .await
    .unwrap();
    let body = tokio::fs::read_to_string(&path).await.unwrap();
    let entry: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    assert_eq!(entry["session_id"], "11111111-2222-3333-4444-555555555555");
}

#[tokio::test]
async fn audit_omits_session_id_when_none() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("audit.jsonl");
    let log = AuditLog::open(&path).await.unwrap();
    log.write(AuditRecord {
        ts: "2026-01-01T00:00:00Z".into(),
        mode: "live",
        tool: "observe".into(),
        args: serde_json::json!({}),
        ok: true,
        duration_ms: Some(5),
        error: None,
        vision_fallback: None,
        session_id: None,
    })
    .await
    .unwrap();
    let body = tokio::fs::read_to_string(&path).await.unwrap();
    let entry: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    assert!(entry.get("session_id").is_none(),
        "session_id should be omitted when None for stdio mode: {entry}");
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test --workspace audit
```

Expected: compile error — `session_id` not a field of `AuditRecord`.

- [ ] **Step 3: Add the field**

In `src/audit.rs`, extend the struct:

```rust
#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub ts: String,
    pub mode: &'static str,
    pub tool: String,
    pub args: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_fallback: Option<bool>,
    /// Daemon-mode session attribution (ADR-0012). `None` in stdio mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}
```

Update every existing constructor / call site of `AuditRecord` (likely `src/server.rs::dispatch_audited` is the only producer) to set `session_id: None` until task 5.4 wires it from the transport.

- [ ] **Step 4: Run tests**

```
cargo test --workspace audit_includes_session_id audit_omits_session_id
```

Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add src/audit.rs src/server.rs
git commit -m "feat(audit): optional session_id field per ADR-0012 (phase 5)"
```

## Task 5.3: Thread `SessionId` through the daemon's MCP handler

**Files:**
- Modify: `src/transport/daemon.rs`
- Modify: `src/server.rs` (add `dispatch_audited_for_session` helper)

- [ ] **Step 1: Write the failing test**

Append to `tests/integration/daemon_http.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn audit_contains_session_id_for_http_call() {
    let tmp = tempfile::tempdir().unwrap();
    let audit_path = tmp.path().join("audit.jsonl");
    let server = test_support::test_server_with_audit_path(&audit_path).await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "tok").await;
    let client = reqwest::Client::new();

    // initialize
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer tok")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
        .send().await.unwrap();
    let mcp_sid = resp.headers().get("mcp-session-id").cloned().unwrap();

    // call observe
    client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer tok")
        .header("mcp-session-id", mcp_sid)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"observe","arguments":{"detail":"summary"}}}))
        .send().await.unwrap();

    // Give the audit-writer a moment to flush.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.unwrap();
    let last = body.lines().rev().find(|l| l.contains("\"tool\":\"observe\"")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(last).unwrap();
    assert!(entry["session_id"].as_str().is_some(),
        "expected session_id on observe call: {entry}");
}
```

Add a `test_server_with_audit_path` helper that uses the given path.

- [ ] **Step 2: Run to verify failure**

Expected: the call may compile but the audit entry has no `session_id` because the transport layer isn't populating it.

- [ ] **Step 3: Wire `session_id` through rmcp → server**

The rmcp `StreamableHttpService` factory closure is called once per session and gets the session id from rmcp's `SessionManager`. The cleanest threading is to clone an `Arc<SessionRegistry>` into the factory closure, mint a `SessionId` per call, and stash it on a per-server-instance field. Concretely, change the closure in `daemon.rs` to:

```rust
let registry = SessionRegistry::default();
let registry_for_service = registry.clone();
let mcp_service = StreamableHttpService::new(
    move || {
        let (session_id, cancel) = registry_for_service.open();
        let mut srv = (*server).clone();
        srv.set_session(session_id, cancel);   // see step 4
        Ok(srv)
    },
    LocalSessionManager::default().into(),
    Default::default(),
);
```

- [ ] **Step 4: Add `set_session` to `KdeMcp`**

In `src/server.rs`, add a field and accessor:

```rust
pub struct KdeMcp {
    ...existing fields...
    /// Daemon-mode only: the session this instance is bound to. None
    /// in stdio mode.
    pub session_id: Option<crate::transport::session::SessionId>,
    pub session_cancel: Option<tokio_util::sync::CancellationToken>,
}

impl KdeMcp {
    pub fn set_session(
        &mut self,
        id: crate::transport::session::SessionId,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        self.session_id = Some(id);
        self.session_cancel = Some(cancel);
    }
}
```

Initialise both to `None` in `new_with_cancel`. Update `dispatch_audited` to write `session_id: self.session_id.map(|s| s.to_string())` into the `AuditRecord`.

- [ ] **Step 5: Run tests**

```
cargo test --test daemon_http audit_contains_session_id_for_http_call -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/transport/daemon.rs src/server.rs tests/integration/daemon_http.rs
git commit -m "feat(daemon): mint SessionId per HTTP connection + audit attribution (phase 5)"
```

## Task 5.4: Two clients concurrent observe

**Files:**
- Test: `tests/integration/daemon_http.rs`

- [ ] **Step 1: Write the multi-session integration test**

```rust
#[tokio::test(flavor = "multi_thread")]
async fn two_concurrent_clients_get_distinct_session_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let audit_path = tmp.path().join("audit.jsonl");
    let server = test_support::test_server_with_audit_path(&audit_path).await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "tok").await;

    let run = |i: u32| async move {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .header("Authorization", "Bearer tok")
            .json(&serde_json::json!({"jsonrpc":"2.0","id":i,"method":"initialize",
                "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
            .send().await.unwrap();
        let mcp_sid = resp.headers().get("mcp-session-id").cloned().unwrap();
        client
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .header("Authorization", "Bearer tok")
            .header("mcp-session-id", mcp_sid)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":i+100,"method":"tools/call",
                "params":{"name":"observe","arguments":{"detail":"summary"}}}))
            .send().await.unwrap();
    };
    let (_, _) = tokio::join!(run(1), run(2));

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.unwrap();
    let observe_entries: Vec<_> = body.lines()
        .filter(|l| l.contains("\"tool\":\"observe\""))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect();
    assert!(observe_entries.len() >= 2, "expected ≥2 observe entries, got {}", observe_entries.len());
    let sids: std::collections::HashSet<String> = observe_entries.iter()
        .filter_map(|e| e["session_id"].as_str().map(String::from))
        .collect();
    assert_eq!(sids.len(), 2, "expected 2 distinct session_ids, got {:?}", sids);
}
```

- [ ] **Step 2: Run**

```
cargo test --test daemon_http two_concurrent_clients_get_distinct_session_ids -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/integration/daemon_http.rs
git commit -m "test(daemon): two concurrent clients get distinct session_ids (phase 5)"
```

---

# Phase 6 — Idle stop + lifecycle polish

End of phase: daemon idles down after 10 min with zero connections, exits 0; SIGTERM drains with 3-sec deadline; lifecycle events appear in audit.

## Task 6.1: Track active connections + idle countdown

**Files:**
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Add an active-connections counter**

Extend `DaemonState`:

```rust
#[derive(Clone)]
struct DaemonState {
    started_at: Instant,
    sessions: Arc<dashmap::DashMap<uuid::Uuid, ()>>,
    server: Arc<KdeMcp>,
    active_conns: Arc<std::sync::atomic::AtomicU64>,
    last_active_at: Arc<parking_lot::Mutex<Instant>>,
}
```

(Add `parking_lot = "0.12"` to `Cargo.toml` if not present.)

Wrap the rmcp service with a layer that bumps the counter on connection start and decrements on close. The simplest hook is an axum middleware on the `/mcp` router:

```rust
async fn conn_tracking_middleware(
    State(state): State<DaemonState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    state.active_conns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *state.last_active_at.lock() = Instant::now();
    let resp = next.run(req).await;
    state.active_conns.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    *state.last_active_at.lock() = Instant::now();
    resp
}
```

Mount it as a layer on the `mcp_router`.

## Task 6.2: 10-min idle timeout

**Files:**
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests/integration/daemon_http.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn daemon_idles_down_after_timeout() {
    let server = test_support::test_server().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // Use a 200ms idle window for the test.
    let idle = std::time::Duration::from_millis(200);
    let handle = tokio::spawn(async move {
        kde_mcp::transport::daemon::serve_with_listener_and_timeout(
            server,
            listener,
            None,
            idle,
        )
        .await
    });
    // Make one request to keep alive briefly.
    let _ = reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await.unwrap();
    // Now wait > idle window. The daemon should exit.
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("daemon should exit within 2s of idle timeout");
    assert!(result.is_ok(), "daemon task panicked: {:?}", result);
}
```

- [ ] **Step 2: Add `serve_with_listener_and_timeout`**

Extend `daemon.rs` so the public surface is:
- `serve(server)` — production: reads token, uses default 600s idle.
- `serve_with_listener(server, listener)` — test entry point with default 600s idle.
- `serve_with_listener_and_token(...)` — token-aware.
- `serve_with_listener_and_timeout(server, listener, token, idle)` — full control.

Inside `serve_with_listener_and_timeout`, spawn a watchdog task:

```rust
let stop = tokio_util::sync::CancellationToken::new();
let stop_for_watchdog = stop.clone();
let state_for_watchdog = state.clone();
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if state_for_watchdog.active_conns.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            let last = *state_for_watchdog.last_active_at.lock();
            if last.elapsed() >= idle {
                tracing::info!("daemon idle for {:?}, stopping", idle);
                stop_for_watchdog.cancel();
                return;
            }
        }
    }
});

let server_handle = axum::serve(listener, app).with_graceful_shutdown(async move {
    stop.cancelled().await;
});
server_handle.await?;
Ok(())
```

- [ ] **Step 3: Run**

```
cargo test --test daemon_http daemon_idles_down_after_timeout -- --nocapture
```

Expected: PASS in <2s.

- [ ] **Step 4: Commit**

```bash
git add src/transport/daemon.rs tests/integration/daemon_http.rs Cargo.toml Cargo.lock
git commit -m "feat(daemon): 10-min idle-stop with watchdog task (phase 6)"
```

## Task 6.3: SIGTERM drain

**Files:**
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Listen for SIGTERM, cancel the same `stop` token**

In `serve_with_listener_and_timeout`, before the watchdog spawn, add:

```rust
let stop_for_sig = stop.clone();
tokio::spawn(async move {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    term.recv().await;
    tracing::info!("daemon: SIGTERM received, draining (3s)");
    stop_for_sig.cancel();
});
```

`with_graceful_shutdown` already drains in-flight requests; axum will wait for handlers to return for up to its default. To cap drain at 3 seconds, wrap the await with `tokio::time::timeout`:

```rust
let _ = tokio::time::timeout(Duration::from_secs(3), server_handle).await;
```

- [ ] **Step 2: Manual smoke test**

```bash
cargo build --release
~/.local/bin/kde-mcp --mode=live --transport=daemon &
PID=$!
sleep 1
curl -s http://127.0.0.1:7575/healthz   # expect ok
kill -TERM $PID
wait $PID
echo "exit: $?"
```

Expected: process exits within 3 seconds, exit 0.

- [ ] **Step 3: Commit**

```bash
git add src/transport/daemon.rs
git commit -m "feat(daemon): SIGTERM drain with 3s deadline (phase 6)"
```

## Task 6.4: Audit lifecycle events

**Files:**
- Modify: `src/audit.rs`
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Add a `write_event` helper on AuditLog**

In `src/audit.rs`:

```rust
impl AuditLog {
    /// Write a lifecycle event (daemon_start, daemon_idle_stop,
    /// session_open, session_close, auth_failure, cancel_all). The
    /// `event` becomes the `tool` field for cheap log greppability.
    pub async fn write_event(&self, event: &str, extra: Value) -> std::io::Result<()> {
        let rec = AuditRecord {
            ts: chrono::Utc::now().to_rfc3339(),
            mode: "live",
            tool: event.to_string(),
            args: extra,
            ok: true,
            duration_ms: None,
            error: None,
            vision_fallback: None,
            session_id: None,
        };
        self.write(rec).await
    }
}
```

- [ ] **Step 2: Emit `daemon_start` and `daemon_idle_stop`**

In `daemon.rs::serve_with_listener_and_timeout`:

```rust
state.server.audit.write_event("daemon_start", json!({})).await.ok();
// ... after server_handle.await:
state.server.audit.write_event("daemon_idle_stop", json!({})).await.ok();
```

(Adjust the `state.server.audit` access if `audit` isn't directly accessible; the field exists on `KdeMcp`.)

- [ ] **Step 3: Emit `session_open` and `session_close`**

In the rmcp factory closure, after `registry.open()`:

```rust
let audit = server.audit.clone();
let sid = session_id;
tokio::spawn(async move {
    audit.write_event("session_open", serde_json::json!({"session_id": sid.to_string()})).await.ok();
});
```

For `session_close`, the closure that drops the per-session state when the rmcp connection closes is best registered via the `LocalSessionManager`'s session lifecycle hooks. If the API doesn't expose a per-session-close hook in rmcp 1.7, instead emit `session_close` from a `Drop` on a per-session guard stored on `KdeMcp::set_session` — i.e., wrap the audit emit in a tiny struct:

```rust
struct SessionCloseGuard {
    sid: SessionId,
    audit: Arc<AuditLog>,
}
impl Drop for SessionCloseGuard {
    fn drop(&mut self) {
        let sid = self.sid;
        let audit = self.audit.clone();
        tokio::spawn(async move {
            audit.write_event("session_close", serde_json::json!({"session_id": sid.to_string()})).await.ok();
        });
    }
}
```

Stash one on the per-session `KdeMcp`.

- [ ] **Step 4: Write integration test**

Append to `tests/integration/daemon_http.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_events_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let audit_path = tmp.path().join("audit.jsonl");
    let server = test_support::test_server_with_audit_path(&audit_path).await;
    let port = test_support::spawn_daemon_with_auth(server, "127.0.0.1:0", "tok").await;

    let client = reqwest::Client::new();
    client.post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer tok")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
        .send().await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let body = tokio::fs::read_to_string(&audit_path).await.unwrap();
    assert!(body.contains("\"tool\":\"daemon_start\""), "daemon_start missing");
    assert!(body.contains("\"tool\":\"session_open\""),  "session_open missing");
}
```

- [ ] **Step 5: Run**

```
cargo test --test daemon_http lifecycle_events_recorded -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/audit.rs src/transport/daemon.rs tests/integration/daemon_http.rs
git commit -m "feat(daemon): audit daemon_start/session_open/session_close (phase 6)"
```

---

# Phase 7 — Tray + cancel hookup

End of phase: in daemon mode the tray shows session count and active tool; "Abort current session" cancels only the most-recently-active session; "Abort all" cancels every in-flight call across every session; Meta+Esc fires "Abort all".

## Task 7.1: Tray runs only in daemon mode

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Gate `tray::spawn` on transport**

Replace the unconditional `kde_mcp::tray::spawn(server.tray.clone())` in `main.rs` with:

```rust
if cli.transport == kde_mcp::cli::Transport::Daemon {
    kde_mcp::tray::spawn(server.tray.clone());
}
```

In stdio mode there's no tray. The cancel UX in stdio mode is unchanged (Meta+Esc via `cancel.rs` still works since it's per-process).

- [ ] **Step 2: Smoke test**

Run stdio mode and confirm no tray icon appears:

```bash
~/.local/bin/kde-mcp --mode=live --transport=stdio < /dev/null &
sleep 1
# visually confirm no kde-mcp tray icon in Plasma
kill %1
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(tray): only spawn tray in daemon mode (phase 7)"
```

## Task 7.2: Tooltip shows session count + active tool

**Files:**
- Modify: `src/tray.rs`
- Modify: `src/transport/daemon.rs`

- [ ] **Step 1: Extend TrayState**

In `src/tray.rs`:

```rust
pub struct TrayState {
    pub in_flight: AtomicUsize,
    pub current_tool: Mutex<Option<String>>,
    pub quit_requested: AtomicBool,
    pub cancel: CancellationToken,
    /// Daemon mode: number of concurrent sessions. Stays 0 in stdio.
    pub sessions: AtomicUsize,
}
```

Update `tool_tip` to include `sessions` in the title when `> 0`:

```rust
let title = match (in_flight, sessions) {
    (0, 0) => "kde-mcp — idle".into(),
    (0, n) => format!("kde-mcp — idle, {n} session(s) connected"),
    (k, _) => format!("kde-mcp — running {} ({k} in flight)",
        current.as_deref().unwrap_or("(tool)")),
};
```

- [ ] **Step 2: Bump `sessions` from the daemon registry**

In `src/transport/daemon.rs`, after `registry.open()`:

```rust
server.tray.sessions.fetch_add(1, Ordering::Relaxed);
// in the SessionCloseGuard::drop:
server.tray.sessions.fetch_sub(1, Ordering::Relaxed);
```

(Store a `Arc<TrayState>` clone on the close guard for the decrement.)

- [ ] **Step 3: Manual visual check**

Run daemon mode, point Claude Code at it, tray tooltip should read `kde-mcp — idle, 1 session(s) connected`. Open another session: `2 session(s) connected`.

- [ ] **Step 4: Commit**

```bash
git add src/tray.rs src/transport/daemon.rs
git commit -m "feat(tray): show session count + per-session activity (phase 7)"
```

## Task 7.3: Per-session vs. global cancel tokens

**Files:**
- Modify: `src/cancel.rs`
- Modify: `src/tray.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Replace single CancellationToken with shared + per-session**

In `src/server.rs`'s `KdeMcp`, keep the existing `cancel: CancellationToken` as the **shared** "abort all" token. Use the per-session `session_cancel: Option<CancellationToken>` (added in task 5.3) as the per-session token.

Wherever `dispatch_audited` `select!`s on cancellation, wait on BOTH:

```rust
tokio::select! {
    res = fut => res,
    _ = self.cancel.cancelled() => Err(ToolError::Cancelled { ... }),
    _ = async {
        if let Some(c) = &self.session_cancel { c.cancelled().await }
        else { std::future::pending::<()>().await }
    } => Err(ToolError::Cancelled { ... }),
}
```

- [ ] **Step 2: Wire Meta+Esc to fire the shared token only**

`src/cancel.rs::install` already returns the shared token. No change needed here; it's the same `cancel` field used by both stdio and daemon paths.

- [ ] **Step 3: Add "Abort current session" tray menu item**

In `src/tray.rs::menu`, add a second StandardItem above the existing Abort:

```rust
StandardItem {
    label: "Abort current session".into(),
    icon_name: "process-stop".into(),
    enabled: in_flight > 0,
    activate: Box::new(|t: &mut Self| t.state.abort_current_session()),
    ..Default::default()
}.into(),
MenuItem::Separator,
```

Provide `TrayState::abort_current_session()`: looks up "most recently active session" — track via an `AtomicI64` storing the millis-since-epoch of the last in-flight start, plus a `Mutex<Option<SessionId>>` set when a tool call begins. The session-id-tracking is done in `dispatch_audited` before `select!`.

Or, simpler: track the most recent in-flight session in `TrayState::current_session: Mutex<Option<SessionId>>`. The dispatch path sets it when starting, clears on finish. `abort_current_session` reads it and fires the matching session's cancel via the `SessionRegistry::cancel(id)`.

The registry needs to be reachable from `TrayState`. Plumb it: `TrayState` gets an `Option<Arc<SessionRegistry>>` that's set in daemon mode at startup.

- [ ] **Step 4: Commit**

```bash
git add src/cancel.rs src/server.rs src/tray.rs
git commit -m "feat(cancel): per-session tokens + 'Abort current session' tray item (phase 7)"
```

---

# Phase 8 — Install script + runbook updates

End of phase: `./packaging/install.sh` brings the daemon up end-to-end on a fresh box. Runbook understands which transport is active.

## Task 8.1: Install-script changes

**Files:**
- Modify: `packaging/install.sh`

- [ ] **Step 1: Add `--transport=` flag parsing**

In the argument-parsing loop near the top, add a `TRANSPORT` variable defaulting to `daemon` if `$XDG_CURRENT_DESKTOP` matches `KDE` and `$XDG_SESSION_TYPE == wayland`, else `stdio`:

```bash
TRANSPORT="${TRANSPORT:-}"
# ... in the case loop:
        --transport=*) TRANSPORT="${1#*=}"; shift ;;
# ... after parsing:
if [[ -z "$TRANSPORT" ]]; then
    if [[ "$XDG_CURRENT_DESKTOP" =~ KDE ]] && [[ "$XDG_SESSION_TYPE" == "wayland" ]]; then
        TRANSPORT="daemon"
    else
        TRANSPORT="stdio"
    fi
fi
```

- [ ] **Step 2: Install systemd units in daemon mode**

After the binary copy, if `TRANSPORT == daemon`:

```bash
if [[ "$TRANSPORT" == "daemon" ]]; then
    install -D -m 0644 packaging/kde-mcp.socket  "$HOME/.config/systemd/user/kde-mcp.socket"
    install -D -m 0644 packaging/kde-mcp.service "$HOME/.config/systemd/user/kde-mcp.service"
    systemctl --user daemon-reload
    systemctl --user enable --now kde-mcp.socket
    # Force-spawn to make the token file exist for the next step.
    curl -fsS http://127.0.0.1:7575/healthz >/dev/null
fi
```

- [ ] **Step 3: Rewrite the Claude Code MCP config**

Replace the existing `claude mcp add` invocation with a branch:

```bash
if [[ $SKIP_MCP -eq 0 ]]; then
    claude mcp remove kde-mcp 2>/dev/null
    if [[ "$TRANSPORT" == "daemon" ]]; then
        TOKEN="$(cat "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/kde-mcp/token")"
        claude mcp add --transport http kde-mcp http://127.0.0.1:7575/mcp \
            --header "Authorization: Bearer $TOKEN"
    else
        claude mcp add --transport stdio kde-mcp -- "$HOME/.local/bin/kde-mcp" --mode=live --transport=stdio
    fi
fi
```

- [ ] **Step 4: Post-install message**

Replace the existing closing message with a transport-aware variant:

```bash
case "$TRANSPORT" in
  daemon)
    echo "Done (daemon mode)."
    echo "  Verify: curl -fsS http://127.0.0.1:7575/healthz"
    echo "  Logs:   journalctl --user -u kde-mcp.service -f"
    echo "  Stop:   systemctl --user stop kde-mcp.socket"
    ;;
  stdio)
    echo "Done (stdio mode)."
    echo "  Restart your MCP client session to pick up the new server."
    ;;
esac
```

- [ ] **Step 5: Smoke test**

```bash
./packaging/install.sh --skip-mcp --transport=daemon
systemctl --user is-active kde-mcp.socket   # active
curl -fsS http://127.0.0.1:7575/healthz     # JSON
cat ~/.config/systemd/user/kde-mcp.socket   # listed correctly
```

- [ ] **Step 6: Commit**

```bash
git add packaging/install.sh
git commit -m "feat(install): --transport=daemon installs units + writes HTTP MCP config (phase 8)"
```

## Task 8.2: Runbook updates

**Files:**
- Modify: `docs/runbook/AUTONOMOUS-VERIFICATION.md`
- Modify: `docs/runbook/KNOWN-FIXES.md`

- [ ] **Step 1: Add Layer 0 transport detection**

In `AUTONOMOUS-VERIFICATION.md`, add a section before Layer 1:

```markdown
## Layer 0 — transport mode detection

### L0.1 — detect active transport
**Goal:** Record whether kde-mcp is in stdio or daemon mode for this run.
**Pre:** none.
**Depends-on:** (none)
**Steps:**
1. `bash`: `systemctl --user is-active kde-mcp.socket 2>/dev/null || echo inactive`
2. If output is `active`: set `transport=daemon` in `env.json`.
3. Else: set `transport=stdio`.
**Accept:** `env.json.transport` is one of `"daemon"` or `"stdio"`.
**Artefacts:** Adds the `transport` key to `env.json`.
**On fail:** log + continue with `transport=stdio` as default.
```

Adjust L1.4's accept-rule: in daemon mode, the `claude mcp list` line shows `http://127.0.0.1:7575/mcp` instead of a binary path.

Update L1.5 (process count): in daemon mode, expect exactly 1 kde-mcp process (the daemon). In stdio mode, N (one per Claude Code session).

- [ ] **Step 2: Add F10 + F11 known-fix entries**

In `KNOWN-FIXES.md`:

```markdown
| F10 | `401 Unauthorized` from `http://127.0.0.1:7575/mcp` | (none) | no | The token in Claude Code's MCP config is stale. Re-run `packaging/install.sh --transport=daemon` to regenerate it. |
| F11 | `connection refused.*127.0.0.1:7575` | `systemctl --user start kde-mcp.socket` | yes | Daemon socket not bound. Restart the user unit. |
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/KNOWN-FIXES.md
git commit -m "docs(runbook): Layer-0 transport detection + F10/F11 (phase 8)"
```

## Task 8.3: Flip ADR-0012 status to Accepted

**Files:**
- Modify: `docs/adr/0012-singleton-daemon-http.md`
- Modify: `docs/adr/README.md`

- [ ] **Step 1: Update ADR status header**

In `docs/adr/0012-singleton-daemon-http.md`, change `**Status:** Proposed (2026-05-15)` to:

```
**Status:** Accepted (<today's date>)
```

In `docs/adr/README.md` index, change the ADR-0012 row's Status from `Proposed` to `Accepted`.

- [ ] **Step 2: Commit**

```bash
git add docs/adr/0012-singleton-daemon-http.md docs/adr/README.md
git commit -m "docs(adr): mark ADR-0012 Accepted (phase 8 complete)"
```

---

# Self-review checklist (for the executor at end of run)

After all phases land:

- [ ] `cargo test --workspace` — every test green.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- [ ] `cargo fmt --check` — clean.
- [ ] `claude mcp list | grep kde-mcp` shows `✓ Connected` against `http://127.0.0.1:7575/mcp`.
- [ ] 3 concurrent Claude Code sessions → **1** kde-mcp process, **1** tray icon, **1** AT-SPI subscription. Verified by `pgrep -af kde-mcp` and visual inspection of the system tray.
- [ ] Re-run the runbook (`docs/runbook/AUTONOMOUS-VERIFICATION.md`). Layer 0 detects daemon; L1.5 expects exactly 1; all Layer 2 tool calls go through HTTP and pass.
- [ ] `journalctl --user -u kde-mcp.service` shows `daemon_start`, `session_open`, `session_close` events for each connection.
- [ ] `~/.local/state/kde-mcp/audit.jsonl` has `session_id` populated on every tool entry.
- [ ] `kill -TERM $(pgrep -x kde-mcp)` causes a clean exit within 3 seconds.
- [ ] Stdio mode still works: `kde-mcp --mode=live --transport=stdio < /dev/null` boots, accepts MCP frames, exits on EOF.
