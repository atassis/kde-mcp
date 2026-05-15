# ADR-0012: Singleton daemon with HTTP transport for multi-session desktop use

**Status:** Proposed (2026-05-15)

Additive to [[0011-multi-session-lease]]: the lease semantics are unchanged. This ADR adds a long-running daemon process that replaces the per-Claude-Code-session subprocess explosion. Stdio mode is preserved as a co-equal peer for headless/CI use.

Full design: `docs/specs/2026-05-15-singleton-daemon-design.md`.

## Context

ADR-0011 retired the global live-lock that previously serialised kde-mcp processes, in favour of a per-session mutation lease. That removed the *startup* contention failure mode (the 2026-05-15-1549 runbook regression), but it preserved — and made more visible — a different problem: every Claude Code session still spawns its own `kde-mcp` subprocess over stdio. With four concurrent sessions in normal daily use, the user sees four SNI tray icons, four AT-SPI subscriptions, four KWin script connections, four EIS seats, and four parallel audit logs. The lease coordinated *mutations*, not *processes*.

The 2026-05-15-1909 runbook surfaced this concretely (see `docs/runbook/runs/2026-05-15-1909/summary.md` and the user's session feedback at the same date). The proliferation was tolerable while there was a working coordination story for mutations, but it's an ongoing UX annoyance the user explicitly wants fixed structurally rather than patched.

The natural fix is *one* kde-mcp process per user, serving multiple Claude Code sessions concurrently. MCP's stdio transport is point-to-point (one stdin/stdout pair per connection), so a singleton can't be served over stdio. The MCP protocol's other transport — Streamable HTTP, the SSE-deprecation successor — is the path forward. Claude Code already supports it (`claude mcp add --transport http <url>`), so no client-side work is needed beyond one MCP config migration.

Three alternative shapes were considered for collapsing to one process (full discussion in spec §12):

- **Stdio shim + Unix-socket daemon** — preserves the existing MCP config but ships a 50-line forwarder binary whose only job is to copy bytes. Doubles install surface; introduces a layer that can never go away. Rejected.
- **Soft-singleton tray via D-Bus name ownership** — first kde-mcp child to start grabs `org.kde.kde_mcp.Tray`; others skip the tray spawn. Solves only the visible tray-clutter symptom; leaves the per-session AT-SPI / KWin / EIS / audit duplication in place. Rejected as a half-fix.
- **systemd-user activate-on-demand stdio** — `Accept=true` socket activation spawning a fresh stdio process per connection. Still per-session process; doesn't address the underlying multiplicity. Rejected.

A second-order question was whether to remove stdio mode entirely once the daemon exists. Rejected: stdio remains useful for headless server-OS deployments and CI smoke tests where there is no graphical session, no `$XDG_RUNTIME_DIR` at the expected path, and no use for a tray. The two modes serve genuinely different deployment shapes.

## Decision

**Add `--mode=daemon` alongside `--mode=stdio`. Both are first-class peers.**

Daemon mode:

1. Serves MCP over HTTP on TCP loopback `127.0.0.1:7575`. Activated lazily by systemd-user socket activation (one `.socket` + one `.service` unit, both shipped by `packaging/install.sh`).
2. Authenticates every `POST /mcp` request with a bearer token from `$XDG_RUNTIME_DIR/kde-mcp/token` (mode 0600). Token is generated on first launch.
3. Mints a fresh session_id (UUID v4) per HTTP connection. The session_id threads through the existing ADR-0011 lease record (no schema change), the audit log (new optional field), and the per-session cancel-token registry.
4. Owns exactly one SNI tray icon for the user. Tooltip shows aggregate session activity; the menu's existing "Abort all" item fires a shared cancel token across all sessions, plus a new "Abort current session" item for the most-recently-active session.
5. Idles down after 10 minutes of zero connections (audit-logged `daemon_idle_stop`, clean exit 0). Socket activation re-spawns on the next connection; clients see no errors thanks to the kernel's accept queue.
6. Drains in-flight calls on `SIGTERM` with a 3-second deadline.

Stdio mode is unchanged. The two modes never share in-memory state, but they share on-disk paths (lease.json, approvals.json, audit.jsonl). A user can run both in parallel (e.g. daemon for daily desktop use + stdio child for the runbook smoke test); ADR-0011's lease semantics serialise mutations across them just as they do across sessions within the daemon.

The Claude Code MCP config migrates from `--transport stdio --command .../kde-mcp` to `--transport http http://127.0.0.1:7575/mcp --header "Authorization: Bearer <token>"`. The install script automates this; rollback is documented.

The implementation is phased into 8 independently-commitable steps in spec §13.

## Consequences

**Positive:**

- One tray icon regardless of session count.
- One AT-SPI / KWin / EIS subscription per user; warmer caches; fewer N-way subscription races.
- One audit log, with session_id attribution per entry — trivial post-hoc forensics.
- The runbook's "exactly 1 live kde-mcp process" assertion (L1.5) becomes correct again, by construction.
- Future MCP-over-Unix-socket support (if/when Claude Code adds it) drops in with no daemon-side changes.

**Negative:**

- Adds ~300-500 LOC (HTTP server, auth middleware, session registry, idle-stop loop) plus two systemd unit files and three install-script branches.
- Adds two crates to `Cargo.toml` (`axum`, `listenfd`).
- One-time visible MCP config migration for existing users. The install script handles it, but the config diff is user-visible.
- Daemon lifetime is decoupled from any individual Claude Code session — debugging requires `journalctl --user -u kde-mcp.service` rather than inspecting a child of a known Claude Code PID. systemd's discoverability covers the gap.
- TCP loopback is reachable by any local process running as the user's UID. Bearer-token auth + UID-restricted `$XDG_RUNTIME_DIR` gives defense-in-depth, but the surface is broader than stdio's "only the parent can write to my pipe." This is the explicit cost of singleton-ness.

**Neutral but worth knowing:**

- The bearer token doesn't auto-rotate. Suspected leak → delete the token file, restart the service, re-run install. Documented; not automated in v1.
- Idle-stop timeout (10 min) is a compile-time constant. Not a CLI flag; tuning would require recompile. Deliberate — one less thing to misconfigure.
- The lease record's `holder_session_id` field already accommodates per-session attribution from ADR-0011; daemon mode just populates it from a different source (HTTP-connection-derived UUID instead of process-startup-derived UUID).

## Alternatives considered

(See spec §12 for full discussion.)

- Stdio thin-client + Unix-socket daemon — rejected: dead-weight forwarder binary.
- Soft-singleton tray via D-Bus name — rejected: half-fix, leaves underlying multiplicity.
- systemd-user `Accept=true` activate-per-connection stdio — rejected: still per-session process.
- Dropping stdio mode entirely — rejected: stdio is genuinely useful for headless deployments.

## Date

2026-05-15
