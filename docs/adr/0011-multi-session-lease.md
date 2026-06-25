# ADR-0011: Multi-session coexistence via per-session mutation lease

**Status:** Accepted (2026-05-15)

Supersedes spec §6 step 1 ("Mode lock check") and §11.3 ("Single-session
lock per mode"). Replaces error code `lock_unavailable` with `busy`.
Amends ADR-0010 (Meta+Esc handoff under leases).

## Context

Spec §11.3 mandated a per-mode POSIX flock at process startup
(`$XDG_RUNTIME_DIR/kde-mcp/live.lock`). The intent was a single audit
log, a single "Claude is using your computer" notification, and a single
Meta+Esc cancel target (ADR-0010).

The assumption underneath was *one Claude Code session per user*. That's
wrong. Claude Code spawns one MCP child per session over stdio, so a
user running multiple sessions concurrently has the second one fail at
startup with `lock_unavailable`. Concrete repro: with one session
already holding the live lock, a second session's MCP child failed to
start — L1.4 reported `Failed to connect` because the lock was held by
the first session's process.

Five sessions registered in `claude mcp list` is normal usage on the
target machine. Most are idle relative to the desktop (they're editing
code, not driving the screen). The startup lock penalises them for
existing rather than for acting.

A second, subtler problem: the lock has to protect a *workflow*, not a
*call*. A naïve per-call mutex lets session A `window_control(focus,
W1)`, then session B `window_control(focus, W2)` between A's calls, and
A's next `act_on_element` lands on the wrong window. The lock unit must
match the unit of intent.

## Decision

**No startup lock. Per-session mutation lease with idle timeout.**

### Lease ownership

Stored in `$XDG_RUNTIME_DIR/kde-mcp/lease.json`, accessed under POSIX
flock (`LOCK_SH` for reads, `LOCK_EX` for writes):

```json
{
  "holder_pid": 171055,
  "holder_session_id": "01J5N…",
  "holder_intent": "keyboard.type",
  "acquired_at": "2026-05-15T13:02:04.318Z",
  "last_refresh_at": "2026-05-15T13:02:11.902Z",
  "expires_at":      "2026-05-15T13:02:21.902Z"
}
```

`holder_session_id` is the rmcp session UUID. `holder_intent` is the
wire-name of the most recent tool call (best-effort, for diagnostics
and the `busy` response).

### Acquire / refresh rules

| Caller is current holder? | Caller's tool category | Effect |
|---|---|---|
| n/a (no lease or expired) | mutating | acquire; `expires_at = now + idle_timeout` |
| n/a (no lease or expired) | observing | no lease change |
| yes | mutating | refresh: `last_refresh_at = now`, `expires_at = now + idle_timeout` |
| yes | observing | refresh: same — observation by the holder keeps the lease alive |
| no, lease still valid | mutating | block until `expires_at` or release; on timeout return `busy` |
| no, lease still valid | observing | run free — observation never blocks |

The "observation by the holder refreshes the lease" rule is what makes
typical agent workflows (mutate → screenshot → think → inspect → think
→ mutate) hold the lease across the entire burst without an explicit
`extend()` API.

### Mutating vs observing tool classification

**Mutating** (acquire/refresh lease):
- `keyboard.key`, `keyboard.type`
- `mouse`
- `act_on_element`
- `window_control`
- `clipboard_write`
- `text_editor.edit`
- `bash` (side effects)
- `wait` with `window_appears`, `window_closes`, `screen_change`,
  `text_match` (these block on screen state that other sessions could
  disrupt; the open MCP call itself holds the lease for its duration)

**Observing** (refresh if holder; otherwise lease-neutral):
- `observe`, `inspect`
- `screenshot`, `read_text`
- `window_query`, `clipboard_read`
- `text_editor.view`
- `wait` with `condition=seconds`
- `tools/list`-style introspection

### Release rules

The lease releases on whichever fires first:

1. **Process exit.** kde-mcp's stdin reaches EOF (parent Claude Code
   session exited) → cleanup deletes the lease file under exclusive
   flock. (Tombstone records are unnecessary — a missing file is
   already the canonical "no holder" state per the acquire rules.)
2. **Idle timeout.** `now > expires_at` and the holder hasn't called
   anything in `idle_timeout` seconds. Detected lazily on the next
   acquire attempt from any session — no background timer needed.
3. **Stale holder.** Any reader notices `kill -0 holder_pid` fails →
   treats the lease as expired and overwrites it.
4. **Cancel.** Meta+Esc (or SIGUSR1) hits the holder → in-flight tool
   aborts → release.
5. **Explicit release** *(not in v1 surface, reserved for a future
   `release()` tool if needed)*.

### Idle timeout

Default 10 seconds. Configurable in `~/.config/kde-mcp/policy.toml`:

```toml
[lease]
idle_timeout_seconds = 10  # how long a session keeps the lease idle
acquire_timeout_seconds = 60  # how long a waiting session blocks
```

The 10-second default is a compromise between two failure modes:

- **Too short** (≤5s): an agent thinking for 7s mid-workflow loses the
  lease; a competing session steals it; the original agent's next
  mutation lands in the wrong UI state. This is the exact failure
  ADR-0011 exists to prevent.
- **Too long** (≥30s): a waiting session blocks for 30s after the
  active one is genuinely done. Hurts the multi-session-driving case
  we're trying to enable.

10s covers typical LLM "think between tool calls" intervals (3–8s)
with margin and keeps inter-session waits short. Tune per-user via the
config knob; ship the default.

### `busy` response shape

When a mutation times out waiting for the lease, the tool returns:

```json
{
  "error": {
    "code": "busy",
    "holder_pid": 171055,
    "holder_intent": "keyboard.type",
    "lease_expires_in_ms": 8400,
    "waited_ms": 60000,
    "acquire_timeout_ms": 60000
  }
}
```

The agent can choose to retry (often the right move — the held burst
will end soon) or surface the conflict to the user. The structured
fields are response-as-instruction (ADR-0004) — no prose in the tool
description telling the model what to do.

### Audit log

Single shared `$XDG_RUNTIME_DIR/kde-mcp/audit.jsonl`, opened
`O_APPEND`. POSIX guarantees atomic appends for writes ≤ `PIPE_BUF`
(4096 bytes on Linux), and audit lines fit comfortably under that
ceiling. Each line carries `pid` and `session_id` so multi-session
streams can be filtered post-hoc.

### Notification handoff

The "Claude is using your computer" notification is owned by whichever
process currently holds the lease. On acquire from a cold state (no
prior holder within the last 5s), fire the notification. On release,
schedule a 5s debounce timer; if no new acquire arrives within 5s,
dismiss. Re-acquires within 5s reuse the existing notification — no
flicker between adjacent bursts from the same or different sessions.

The notification body can optionally show the active session's PID for
multi-session disambiguation; default omits it (one user, one piece of
information at a time).

### Meta+Esc handoff (amends ADR-0010)

ADR-0010 registered KGlobalAccel from each kde-mcp process. With N
processes that's a race — only one can own a given shortcut.

Revised: each kde-mcp registers KGlobalAccel **only while it holds the
lease**. Registration happens inside the same acquire path as the
notification; deregistration happens on release. Because the lease
serialises acquires across all sessions, only one process is ever
registered at a time. SIGUSR1 fallback is unchanged (per-process and
always available).

If KGlobalAccel registration fails (kglobalaccel unavailable, or a
brief race during handoff), the holder falls back to SIGUSR1 only —
matches the ADR-0010 "best-effort registration" stance.

## Consequences

- **Five idle sessions cost almost nothing.** No startup contention.
  Each kde-mcp child is inert until its agent makes a call.
- **Active multi-session works fairly.** Bursts of activity from one
  session are atomic; waiting sessions see at worst `idle_timeout +
  call duration` delay before they get their turn.
- **Observation never blocks.** Any session can `observe` /
  `screenshot` / `inspect` regardless of who holds the lease — useful
  for "what is the other agent doing?" debugging too.
- **No new daemon, no new transport.** The lease is one JSON file under
  flock; everything else stays per-process stdio.
- **Spec edits required during implementation:**
  - §6 step 1 (mode lock check) → replace with lease-check on
    mutating-tool dispatch
  - §11.3 (single-session lock per mode) → replace with lease scheme
  - §8 error table: remove `lock_unavailable`, add `busy` with the
    fields above
  - §13 module layout: lease lives in `src/policy.rs` (same place the
    gate already serialises) or a sibling `src/lease.rs` if it grows
- **ADR-0010 dispatch wrapper:** `TrayGuard::enter()` becomes the
  natural place to acquire/refresh the lease (mutating path) or
  refresh-only (observing path with a current holder). Cancellation
  already routes through the same path.
- **Default idle timeout is a real tunable.** Local-model users with
  slow inference may want 30s; users running snappier hosted models may
  want 5s. The default of 10s will not be right for everyone — accept
  this and ship a config knob.

## Alternatives considered

- **Drop the lock entirely; N independent processes with no
  coordination.** Simplest possible, but loses workflow-level
  atomicity — exactly the scenario where A focuses W1, B focuses W2
  between A's calls, A's next click lands on W2. Rejected.
- **Per-call mutex without lease.** Protects each call, not the burst.
  Fails the same focus-race scenario for the same reason. Rejected.
- **Singleton daemon with MCP streamable HTTP transport.** Genuinely
  multi-tenant via MCP session ids. Was the prior proposal in this
  thread. Rejected for v1 because (a) requires verifying rmcp's
  server-side streamable-HTTP support, (b) doubles the binary surface
  with a daemon + shim split, (c) the lease design achieves the same
  user-visible coordination with ~50 lines of file-locking code.
  Reconsider in v2 if observability / cross-session policy state
  becomes a feature requirement.
- **Coordinator daemon (`kde-mcp-coord`) holding only shared state.**
  Adds a second binary and an IPC protocol for a problem that flock +
  a JSON file solves. Rejected.
- **Explicit `begin_session()` / `end_session()` API on agents.**
  Requires every consumer to be lease-aware. Most agents won't be.
  Rejected; the implicit "refresh on any call" rule covers the same
  ground.
- **`extend(seconds)` API for known-long pauses.** Covered in detail in
  the design conversation: every plausible "I need to hold longer"
  case either (a) is already covered by long-running `wait` calls
  holding the lease through their duration, or (b) is covered by the
  observation-refreshes-lease rule, or (c) is rare enough (human-in-
  the-loop with no automated waiting) that `busy` + retry is
  acceptable. Rejected for v1.
- **Force-disconnect / preemption** of the current holder by a waiting
  session. Rejected: disrupting an in-flight mutation mid-keystroke or
  mid-drag leaves the desktop in a state nobody asked for. The user
  flagged this as a bad idea; agreed.
