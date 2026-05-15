# Singleton daemon with HTTP transport — design

**Status:** Draft
**Date:** 2026-05-15
**Author:** atassis
**Audience:** developers implementing the daemon mode; future contributors deciding whether to extend it.

---

## 1. Summary

`kde-mcp` gains a long-running **daemon mode** in addition to the existing **stdio mode**. The daemon listens for MCP traffic over HTTP on `127.0.0.1:7575`, serves any number of concurrent Claude Code (or other MCP-client) sessions, and is started lazily by systemd-user socket activation. The existing stdio mode is preserved as a peer for headless / single-agent / CI use; daemon mode is the default for desktop use.

The motivating problem: under [[ADR-0011]] (per-session mutation lease) each Claude Code terminal now spawns its own `kde-mcp` child, and each child registers its own SNI tray icon, KWin script connection, AT-SPI subscription, and audit log. Four concurrent sessions = four trays + four parallel desktop subscriptions. The lease coordinated *mutations* but did nothing about *process count*. The daemon collapses the per-session process explosion back to one.

---

## 2. Non-goals

- **Not a new transport for the protocol.** We use MCP's standard Streamable HTTP transport (the SSE-deprecation successor), not a bespoke wire format. Claude Code already supports it via `claude mcp add --transport http`.
- **Not a network daemon.** Loopback only. No remote MCP. No cross-user. No firewall hole.
- **Not removing stdio mode.** Stdio stays a first-class peer for headless server-OS use and CI smoke tests.
- **Not changing the tool surface.** No new tools, no schema changes for existing tools. The only externally-visible new thing is the bearer token in the MCP config.
- **Not introducing a separate "client shim" binary.** One binary, two modes (`--mode=stdio` and `--mode=daemon`). No second binary to install, package, or version-skew.

---

## 3. Two co-equal modes

| | `--mode=stdio` | `--mode=daemon` |
|---|---|---|
| Use case | Headless server OS, CI, single-agent | Desktop with one+ Claude Code sessions |
| Transport | MCP over stdio | MCP over HTTP, TCP loopback `127.0.0.1:7575` |
| Lifetime | One client connection → exit on stdin EOF | Long-lived; idle-stop after 10 min with zero connections |
| Tray icon | Off | One icon for the daemon |
| Cancel hotkey (Meta+Esc) | Cancels this process's single in-flight call | Cancels all in-flight calls across all sessions |
| Concurrency | One session at a time | N sessions, all sharing one process |
| Activated by | Claude Code subprocess spawn | systemd-user socket activation |
| Started how | Claude Code MCP config: `command = .../kde-mcp --mode=stdio` | `systemctl --user enable --now kde-mcp.socket` |
| Auth | none (stdin is auth — only the parent can write) | bearer token from `$XDG_RUNTIME_DIR/kde-mcp/token` |
| Audit log holder | per-process file | one file, `session_id` field per entry |

Stdio mode keeps its existing behaviour byte-for-byte: the runbook smoke test (`docs/runbook/AUTONOMOUS-VERIFICATION.md`) and any `claude mcp add --transport stdio …` configuration continue to work unchanged. Daemon mode is additive.

A user can run both at once — a daemon at `127.0.0.1:7575` plus a stdio child for the runbook — but in normal desktop use you pick one and stay there. The two never share state across mode boundaries (the daemon has its own audit log, lease record, and approval map; a stdio child has its own).

---

## 4. Daemon lifecycle

### 4.1 Activation

Two systemd-user units land in `~/.config/systemd/user/`:

**`kde-mcp.socket`**

```ini
[Unit]
Description=kde-mcp daemon socket
PartOf=kde-mcp.service

[Socket]
ListenStream=127.0.0.1:7575
NoDelay=true
# Single accept loop in the daemon; do not Accept= per-connection
Accept=false

[Install]
WantedBy=sockets.target
```

**`kde-mcp.service`**

```ini
[Unit]
Description=kde-mcp daemon (HTTP MCP server)
Requires=kde-mcp.socket
After=kde-mcp.socket graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart=%h/.local/bin/kde-mcp --mode=daemon
Restart=on-failure
RestartSec=2
# Idle timeout: stop the service after 10 min of zero connections.
# Socket-activation will restart it on the next connection.
TimeoutStopSec=5

[Install]
WantedBy=default.target
```

On install: `systemctl --user daemon-reload && systemctl --user enable --now kde-mcp.socket`. The socket binds immediately; the service is not started until the first HTTP connection.

### 4.2 fd inheritance

When activated, systemd binds the TCP socket itself and passes the listening fd to the daemon as fd 3 with `LISTEN_FDS=1` and `LISTEN_PID=$pid`. The daemon uses the `listenfd` crate (or equivalent) to grab fd 3 instead of binding its own. This means:

- Idle-stop and restart are seamless — clients reconnecting during the gap get queued in the kernel's accept queue, no `ECONNREFUSED`.
- The daemon binary itself doesn't need to know what port it's on. Configuration lives in the unit file.
- For development / non-systemd runs, the daemon falls back to binding `127.0.0.1:7575` directly when `LISTEN_FDS` is unset.

### 4.3 Idle stop

The daemon tracks active HTTP connections. When `active_connections == 0` for 10 minutes, it logs `idle_stop` to the audit log and exits 0. systemd treats exit 0 as success and won't restart; the socket unit stays bound, so the next connection re-activates the service.

The 10-minute window is a constant in `src/main.rs` (`const IDLE_TIMEOUT: Duration = Duration::from_secs(600)`). Not a CLI flag — there's no operational reason to tune it from outside the binary, and a flag is one more thing to misconfigure.

### 4.4 Shutdown

`SIGTERM` (sent by `systemctl --user stop` or by logout via `graphical-session.target` going down): drain in-flight calls with a 3-second deadline, then exit. In-flight clients see their request fail with a graceful 503; they're expected to reconnect, which triggers reactivation.

---

## 5. Transport

### 5.1 HTTP server

Stack:

- `axum` for the HTTP server
- `rmcp` (the official Rust MCP SDK) for the Streamable HTTP transport adapter, mounted at `POST /mcp`
- `tokio` runtime (already used for stdio mode)
- `listenfd` for socket-activation fd pickup

The `axum::Router` has exactly two routes:

- `POST /mcp` — the MCP endpoint. `rmcp` handles framing, session establishment, JSON-RPC dispatch.
- `GET /healthz` — liveness probe (returns `200 OK` with `{"ok": true, "uptime_s": N, "sessions": N}`). No auth on this route. Useful for the install script's post-install check and for the runbook's L1-equivalent.

No other routes. The HTTP surface is intentionally tiny.

### 5.2 Concurrency

Each HTTP connection runs on its own `tokio` task. Inside the daemon, tool dispatch already uses an async `dispatch_audited` function that holds the lease for the duration of a mutation; that code is unchanged. Two sessions calling `observe` race freely; two sessions calling `keyboard.type` serialise on the lease. Per ADR-0011.

The daemon's shared state (the `KdeMcp` struct) becomes an `Arc<KdeMcp>` cloned into each task. The existing `KdeMcp` already holds its sub-state behind `Arc<Mutex<...>>` or `Arc<RwLock<...>>` per-field; minimal new locking needed.

### 5.3 Why HTTP and not Unix socket

Claude Code's `--transport http` (verified via `claude mcp add --help` 2026-05-15) accepts an HTTP URL, not a Unix socket path. Unix sockets would have been nicer (filesystem permissions = auth, no port pinning), but Claude Code doesn't speak `http+unix://`. TCP loopback + bearer token is the working substitute. If Claude Code adds Unix-socket support later, we can migrate without changing the daemon internals.

### 5.4 Port

`7575`. No reservation in `/etc/services`; chosen to avoid common dev ports (5xxx, 8xxx, 3xxx). If the port is already bound, the systemd socket unit fails to start with a clear error — preferable to dynamic discovery + a port file (which adds a moving part).

---

## 6. Authentication

### 6.1 Bearer token

On first daemon launch, if `$XDG_RUNTIME_DIR/kde-mcp/token` does not exist, generate a fresh 32-byte random hex string and write it with mode 0600. On subsequent launches, read the existing file. The token survives daemon restarts within a session (the file is in `XDG_RUNTIME_DIR`, tmpfs-backed, cleared at logout).

Every HTTP request to `POST /mcp` must carry `Authorization: Bearer <token>`. Requests without the header, or with a wrong token, get `401 Unauthorized` and an audit-log entry tagged `auth_failure` (one entry per minute per source IP, to avoid log spam on probe loops).

`GET /healthz` is unauthenticated — it returns no sensitive data and is useful for install-time sanity checks.

### 6.2 Why a token

TCP loopback is reachable by any process running as your UID. Without a token:
- A browser tab loaded by anything that can resolve `127.0.0.1:7575` could (in principle, with CORS misconfiguration) drive your desktop.
- Any other local process running as your UID — a developer tool, a misbehaving plugin, a compromised dependency — can issue MCP calls.

With a token in UID-only-readable `$XDG_RUNTIME_DIR`, only processes that can read your runtime dir can authenticate. That's still "any process you run," but it's defense-in-depth against the browser/CORS class of accidents.

### 6.3 Token rotation

Out of scope for v1. If you suspect leak: delete the token file and restart the service; a new token is minted on next start, and you re-run the install script's `claude mcp add` step. A future iteration could auto-rotate on daemon restart and re-write Claude Code's MCP config, but that's not worth the complexity for a single-user dev box.

### 6.4 Install-script changes

`packaging/install.sh` learns a new step: after installing units and starting the socket, it reads (or waits-for-and-reads) the token file, then runs:

```sh
claude mcp remove kde-mcp 2>/dev/null
claude mcp add --transport http kde-mcp http://127.0.0.1:7575/mcp \
    --header "Authorization: Bearer $(cat "$XDG_RUNTIME_DIR/kde-mcp/token")"
```

The token is captured at MCP-config-write time; if it later rotates, the config goes stale and the user must re-run install. Documented in the install script's post-install message.

---

## 7. Per-session attribution

### 7.1 Session ID minting

When `rmcp` accepts a new connection, the daemon mints a fresh `session_id: Uuid::new_v4()` and stores it in the per-connection task's local state. The MCP `initialize` handshake's `clientInfo` is captured alongside (for the audit log).

Sessions are intentionally cheap: no persistent state on disk, no client cooperation needed, no API exposure to the client. The session_id is purely a daemon-internal handle that threads through:

- The lease record (already keyed by `holder_session_id` per ADR-0011 — no schema change).
- The cancel-token registry (§7.3).
- The audit log (one new field).
- The tray tooltip's session count.

When the HTTP connection closes, the daemon drops the in-memory session state but does **not** force-release the lease — it expires lazily per ADR-0011's idle-expiry semantics. This is deliberate: if Claude Code drops mid-mutation (network blip, crash), the lease's natural 10-second expiry beats a force-release that could race a re-issued tool call.

### 7.2 State partitioning

| | Per-session | Shared (daemon-global) |
|---|---|---|
| Lease record | one record at a time, keyed by `holder_session_id` (per ADR-0011) | the lease *file* itself, one per daemon, at `$XDG_RUNTIME_DIR/kde-mcp/lease.json` |
| Cancel token | one `CancellationToken` per session in a `DashMap<SessionId, CancellationToken>` | the shared "cancel all" token used by the tray's Abort-all menu item |
| Audit log entries | each row has `session_id` | one file `$XDG_STATE_HOME/kde-mcp/audit.jsonl` |
| Policy approvals | none | the in-memory `HashMap<class, ApprovalState>` plus the persistent `$XDG_STATE_HOME/kde-mcp/approvals.json` |
| AT-SPI bus connection | none | shared registry connection, reused across sessions |
| KWin script connection | none | shared D-Bus connection, reused |
| EIS (libei) seat | none | shared seat; tool calls take turns via the lease |
| Tray icon + tooltip | none | one icon; tooltip cycles "Idle" / "Session <short-id>: <tool>" |

### 7.3 Cancellation under daemon mode

Per ADR-0010, Meta+Esc is the desktop's "stop everything" panic hotkey. In daemon mode:

- Meta+Esc fires the **shared** cancel token, which is observed by every per-session token via `tokio::select!`. Every in-flight tool call across every session aborts with `cancelled_by_user`.
- The tray menu gets a new "Abort current session" item that fires only the most-recently-active session's token. "Abort all" is the existing item, now wired to the shared token.
- The shared cancel token is one-shot per cycle: after firing, the daemon mints fresh per-session tokens and resets the shared one. (Currently a single global cancel-token field is sufficient; this changes from `Arc<CancellationToken>` to `Arc<Mutex<CancellationToken>>` or equivalent.)

---

## 8. Code layout impact

```
src/
  main.rs                  -- adds --mode=daemon arg; dispatches to mode_stdio() or mode_daemon()
  mode/
    stdio.rs               -- (existing logic, no change beyond extraction)
    daemon.rs               -- NEW. axum router + rmcp Streamable HTTP server + idle-stop loop
  transport/
    auth.rs                 -- NEW. bearer-token middleware for axum
    session.rs              -- NEW. session-id minting, per-session cancel-token registry
  lease.rs                  -- no change
  policy.rs                 -- no change
  audit.rs                  -- add `session_id: Option<Uuid>` field to AuditEntry
  cancel.rs                 -- changes signature: TrayState owns N session tokens + 1 shared
  tray.rs                   -- update tooltip + add "Abort current session" menu item
  tools/*                   -- no changes; tools receive the same KdeMcp state Arc
```

New crates added to `Cargo.toml`:

- `axum` (HTTP server)
- `listenfd` (systemd socket activation)
- `rand` (token generation — may already be present)
- (`rmcp` already in tree from stdio mode)

Tests:

- Unit: `mode/daemon.rs` — token-loading happy path, missing-token-file generates a new one, bad-mode `Authorization` rejected, missing-`Authorization` rejected, `/healthz` works without auth.
- Integration: spawn the daemon on an ephemeral port (using `listenfd::ListenFd::empty` + manual TCP bind), issue MCP `initialize` over HTTP, call `observe`, assert response.
- Multi-session integration: two clients concurrently call `observe` (must succeed), then both call a mutating tool (one wins the lease, the other gets `busy`).
- The existing stdio integration tests run unchanged.

---

## 9. Audit log

The audit record gains a `session_id` field. For daemon mode, every entry includes one. For stdio mode, the field is omitted (or set to `null`) — stdio has no concept of session. The file format is JSON Lines, so an additional optional field is forward-compatible.

The audit-log path is unchanged: `$XDG_STATE_HOME/kde-mcp/audit.jsonl` (i.e. `~/.local/state/kde-mcp/audit.jsonl`). One file per user, append-only, no rotation in v1.

New event types:

- `daemon_start` — daemon booted.
- `daemon_idle_stop` — daemon exited after idle.
- `session_open` — new MCP connection accepted (includes the `clientInfo` block from MCP `initialize`).
- `session_close` — connection dropped.
- `auth_failure` — `Authorization` missing or wrong (rate-limited to 1/min per source).
- `cancel_all` — Meta+Esc fired across sessions.

The runbook's audit-log assertions in `KNOWN-FIXES.md` are unaffected; they grep on tool names, not on these event types.

---

## 10. Migration & install

### 10.1 Install script

`packaging/install.sh` learns a `--mode=stdio|daemon|both` flag, defaulting to `daemon` on a desktop session (`$XDG_SESSION_TYPE == wayland` and `$XDG_CURRENT_DESKTOP` contains `KDE`) and `stdio` otherwise.

- `--mode=daemon`: installs the binary, the two systemd units, the `.desktop` ScreenShot2 authorisation, and rewrites the user's Claude Code MCP config to `--transport http` with the bearer token.
- `--mode=stdio`: installs the binary and the `.desktop` file; rewrites Claude Code config to `--transport stdio`. No systemd units touched.
- `--mode=both`: both, but the Claude Code config still points at exactly one (the daemon by default). The user can manually add a second `claude mcp add` entry under a different name (e.g. `kde-mcp-stdio`) for the runbook to use.

### 10.2 First-run migration

A user upgrading from a previous stdio-only install runs `packaging/install.sh`. The script:

1. Stops any existing systemd-user kde-mcp unit (idempotent — no-op on first run).
2. Installs the binary + units + .desktop.
3. Reloads systemd-user, enables and starts `kde-mcp.socket`.
4. Removes the old Claude Code stdio MCP entry; adds the new HTTP one.
5. Prints "kde-mcp daemon is running. To verify: `curl -s http://127.0.0.1:7575/healthz`."

### 10.3 Rollback

If daemon mode misbehaves: `systemctl --user disable --now kde-mcp.socket`, then re-run `packaging/install.sh --mode=stdio`. The Claude Code MCP config is rewritten to stdio; daemon units stay installed but inert. No state loss — audit log and approvals are in the same paths in either mode.

### 10.4 Runbook

`docs/runbook/AUTONOMOUS-VERIFICATION.md` gains a new Layer-0 step that determines which mode is active (`systemctl --user is-active kde-mcp.service`) and adjusts L1.4's expectation accordingly. The 17 Layer-2 tool calls are mode-agnostic — they go through whatever MCP transport Claude Code is configured for. The "exactly 1 live kde-mcp process" check in L1.5 (already flagged as outdated in the 2026-05-15-1909 run) becomes a real check again: in daemon mode there should be exactly one `kde-mcp --mode=daemon` process during a run.

---

## 11. What this fixes / costs

**Fixes:**

- One tray icon per user, regardless of session count. ✓
- One AT-SPI subscription, one KWin script connection, one EIS seat — warmer caches, fewer flakes from N-way subscription races.
- Unified audit log across sessions; trivial to grep "what did session X do at 14:32" because the session_id is in every entry.
- Eliminates the "another session held the live lock" failure mode for the runbook (already eliminated by ADR-0011, but daemon mode makes it impossible by construction — there's no per-session process to lock).

**Costs:**

- ~300-500 LOC for the HTTP server, auth middleware, session registry, systemd unit files, and tests.
- One-time MCP config migration for existing users. The install script handles it but it's a visible change.
- Daemon process lifetime is decoupled from any individual session — debugging "the daemon" requires `journalctl --user -u kde-mcp.service` instead of "look at the kde-mcp child of my Claude Code process."
- Adds 2 dependencies (`axum`, `listenfd`).

---

## 12. Alternatives considered (and rejected)

### 12.1 stdio thin-client + Unix-socket daemon

A 50-line shim binary speaking stdio that forwards to a Unix-socket daemon. Pro: preserves the existing Claude Code MCP config (`command = .../kde-mcp`). Con: introduces a binary whose only job is to copy bytes between two file descriptors, which is dead weight; doubles the install surface; and gives up the future where MCP-over-Unix-socket becomes a thing (the shim would still be the parent process Claude Code sees, so you can't drop it later).

### 12.2 Soft-singleton via D-Bus name ownership

First kde-mcp child to start owns `org.kde.kde_mcp.Tray` on the session bus; later children skip tray spawning. Pro: minimal change, no transport rework. Con: doesn't fix any of the other per-process resource amplification (KWin script connections, AT-SPI subscriptions, EIS seats), and the audit log stays fragmented. Solves the tray-clutter symptom, not the underlying multi-process architecture.

### 12.3 Activate-on-demand via systemd-user with stdio

Spawn a new `kde-mcp --mode=stdio` per connection via systemd's `Accept=true`. Pro: leaves the stdio code path untouched. Con: still per-session process; per-session tray; per-session subscriptions. Doesn't solve the problem.

### 12.4 Drop stdio mode entirely

Considered and rejected. Stdio is useful for headless / server / CI / single-agent contexts where no graphical session exists. Removing it would force those use cases to spin up a systemd-user instance, which doesn't fit (no graphical session, no `$XDG_RUNTIME_DIR` at the right path, no tray). The two modes serve genuinely different deployment shapes.

---

## 13. Implementation phasing

Suggested sequencing for the eventual implementation plan (writing-plans skill consumes this section):

1. **Plumbing.** Add `--mode=daemon` CLI arg; split `main.rs` into `mode_stdio()` / `mode_daemon()` stubs; daemon stub just logs "daemon mode not yet implemented" and exits. Tests pass in both modes. Cargo features unchanged.
2. **HTTP server scaffold.** axum + listenfd + a `/healthz` route that returns hardcoded JSON. Run via systemd unit files. Verify `curl http://127.0.0.1:7575/healthz` works under socket activation.
3. **rmcp HTTP transport mounted.** `POST /mcp` accepts MCP initialize and routes tool calls to the existing `dispatch_audited`. No auth yet. One-session integration test passes.
4. **Auth middleware.** Token generation, file read, bearer middleware. Tests for happy path + missing/wrong header. Install script learns the token-injection step.
5. **Session registry + audit attribution.** session_id minting, per-session cancel tokens, `session_id` field in audit log. Multi-session integration test (two concurrent `observe`s).
6. **Idle stop + lifecycle polish.** 10-min idle timeout, drain-on-SIGTERM, audit events for daemon_start / daemon_idle_stop / session_open / session_close.
7. **Tray + cancel hookup.** Tooltip shows session count; "Abort current session" menu item; Meta+Esc fires the shared token.
8. **Install script + runbook updates.** `packaging/install.sh --mode=daemon` flow end-to-end. Runbook Layer-0 mode detection. Documentation pass.

Each phase is independently committable. Phase 1 alone is a release-safe no-op; phases 2-3 produce a working but unauthenticated daemon; phases 4-7 finish the design. Phase 8 is the user-facing release.

---

## 14. Open questions

- **`/healthz` token gating?** Currently unauthenticated. A future iteration could require an installed-time-known token only if `auth_required` configured. For v1: unauthenticated, returns only ok/uptime/session-count (no sensitive data).
- **systemd-less environments.** If a user runs Plasma without systemd-user (rare on modern distros, possible on niche derivatives), the install script falls back to `--mode=stdio` and warns. Not a v1 priority.
- **Migration UX on `systemd --user` failure during install.** If `systemctl --user enable` fails (e.g. user-instance not running for the install user), the script should detect, warn, and fall back to stdio mode rather than half-install. Detail for the implementation plan.

---

## 15. Cross-references

- [[../adr/0010-cancel-ux-tray-and-hotkey.md]] — Meta+Esc semantics; updated to cover the per-session vs shared cancel-token split in §7.3.
- [[../adr/0011-multi-session-lease.md]] — per-session mutation lease; daemon mode preserves the lease semantics unchanged.
- ADR-0012 (to be written) — captures the singleton-daemon decision as a single-page rationale.
- [[../runbook/AUTONOMOUS-VERIFICATION.md]] — Layer 0 mode detection step lands when this is implemented.

---

End of spec.
