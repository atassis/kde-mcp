# ADR-0009: Phase 3–6 implementation shape

**Status:** Accepted (2026-05-14). §2 superseded by ADR-0010. §3, §4, §7, §8, §10 lifted 2026-05-15 (see "Deferrals resolved" at the bottom of this file).

## Context

Phases 3–6 were implemented in a single autonomous push after Phase 2.
Several design micro-decisions were taken in flight that aren't covered
by the existing ADRs and need recording so a future reviewer doesn't
have to read commit messages to understand the shape.

## Decisions

### 1. Per-call EIS sessions, not long-running

`src/kwin/eis.rs` opens a fresh `connectToEIS` per tool call (~50ms
handshake) instead of holding a session for the process lifetime. The
per-call form is chosen for: simpler error recovery (a broken EIS
doesn't taint subsequent calls), no shared mutable state across tools,
no Send-isolation lifetime question for the held device. Phase 6 polish
may revisit if latency profiling justifies it.

### 2. Cancel via SIGUSR1 first, KGlobalAccel later

The "Meta+Esc" cancel shortcut (spec §10 layer 11) is wired through a
`tokio_util::CancellationToken` that the policy gate checks on entry,
but the transport in this code is SIGUSR1, not KGlobalAccel. The
shortcut still works (`pkill -USR1 kde-mcp`), but the user-facing
"press Meta+Esc" pitch needs the KGlobalAccel registration which is
non-trivial via zbus (the API has shifted across KDE 5/6 and requires
multi-step setShortcut + signal subscription). Marked as a Phase 6
follow-up; the cancel infrastructure itself is fully in place.

### 3. Approval state is in-process

`PolicyGate::approvals` is a `HashMap<class, ApprovalState>` in
`tokio::sync::Mutex`. Lost on process restart. Spec §15 lists persistent
per-app approval as future work — declined here on YAGNI grounds.

### 4. Approximation: act_on_element target = focused window

Spec §6.0 step 3 says "act_on_element / window_control target = the
ref's owning window." Window→AT-SPI app mapping is non-trivial (window
pid → bus name discovery via Application introspection). We approximate
with the currently-focused window's class. Result: an agent that mints
an element ref via `inspect` and then act_on_element's it must keep the
target focused. `window_control` resolves correctly via the ref's stored
KWin UUID (no AT-SPI mapping needed).

### 5. Bash session uses sentinel-marker protocol

`src/tools/bash.rs` runs commands by writing `<cmd>\nprintf
'%s\\n' "<marker>:$?"\n` to a persistent PTY and reading until the
marker appears. Simpler than parsing prompt boundaries and robust
against shell-specific PS1 customisations. ANSI escapes are stripped
via `strip-ansi-escapes`. Cost: stdout/stderr are merged (a PTY
interleaves them); spec §6.2 lists them separately, so the current
shape returns merged content in `stdout` with `stderr: ""`. Phase 6
polish may demux via a pipe pair.

### 6. Virtual mode uses `dbus-launch`, not `dbus-run-session`

Spec §13 / §14 row 5 mentioned `dbus-run-session`. The current impl
uses `dbus-launch --sh-syntax` instead because it returns the bus
address synchronously without requiring a wrapped child command. Either
approach is valid; this one is slightly simpler to wire.

### 7. Audit log SHA-256 replaces Phase 0 placeholder

`src/audit.rs::placeholder_hash` now produces real SHA-256 (lowercase
hex, `sha256:` prefix). The function name is intentionally unchanged
so the existing call sites all benefit without an API churn — future
cleanup can rename it.

### 8. Audit log rotation deferred

Spec §9 calls for "rotated at 50 MB; keep 5." Not implemented; the
audit log grows unbounded today. Phase 6 polish task; tracked in
`docs/TESTING-CHECKLIST.md`. Operators can rotate manually via
`logrotate` in the meantime.

### 9. Sensitive paths via substring match

`src/util/sensitive_paths.rs` does naive substring matching on the
path (with `~/` expanded). Won't catch obfuscated paths
(`$HOME/.ssh/` without expansion, symlinks, etc.). Sufficient for
"informational warning"; spec §10 layer 9 explicitly says these are
warnings, not blocks.

### 10. `read_text` (OCR) not shipped in this push

Tesseract integration via the `tesseract` Rust binding requires the
`libtesseract-dev` C library headers; binding compilation across distro
versions is brittle. Deferred to a follow-up that introduces an
isolated `examples/tesseract-prober/` sibling first (mirrors the EIS
prober pattern). The wire surface remains a stub returning
`not_implemented`.

## Consequences

- The codebase is feature-complete for Phases 0–5 minus the items
  listed in `docs/TESTING-CHECKLIST.md`.
- Live verification of every tool against the user's real KDE session
  remains. The checklist captures every item that needs a human eye.
- The generalization refactor (a possible future ADR-0010:
  `CompositorBackend` trait) is *not* done. Lots of `kwin::scripting`,
  `kwin::eis`, and `kwin::screenshot` direct calls are still scattered
  through tools. A future ADR can supersede this one when the refactor
  lands.

## Alternatives considered

- **KGlobalAccel first, SIGUSR1 never:** rejected — the registration
  surface is large enough to be its own task.
- **Spec-strict act_on_element target resolution:** rejected for now;
  the focused-window approximation works for the agent's common case.
- **Single combined keyboard+pointer EIS session:** rejected — adds
  state and only saves ~50ms when the agent issues back-to-back
  keyboard+mouse calls (rare).

## Deferrals resolved (2026-05-15)

| Item | Status |
|---|---|
| §1 Per-call EIS sessions | Unchanged. Combined keyboard+pointer in one session now supported via `EisSession::keyboard_device`. |
| §2 KGlobalAccel Meta+Esc | **Done.** See ADR-0010. SIGUSR1 stays as belt-and-braces. |
| §3 Persistent approval state | **Done.** `~/.local/state/kde-mcp/approvals.json` round-trips on grant; loaded at `PolicyGate::new`. Only `Session` decisions persist. |
| §4 act_on_element ref-derived target | **Done.** Element bus name → pid (via `GetConnectionUnixProcessID`) → KWin window with matching pid → resourceClass. Falls back to focused-window heuristic on lookup failure. |
| §5 Bash stdout/stderr demux | Still deferred. Comment in `src/tools/bash.rs` explains the PTY-interleaving + cd-persistence tension. Open as a testing-checklist follow-up. |
| §6 dbus-launch vs dbus-run-session | Unchanged. |
| §7 Audit log SHA-256 | Done (Phase 6). |
| §8 Audit log rotation | **Done.** 50 MB pre-write check rotates to .1/.2/…/.5 per spec §9. |
| §9 Sensitive paths via substring | Unchanged. |
| §10 read_text OCR | **Done.** `tesseract` crate (sync C-FFI in `spawn_blocking`). Optional `include_bboxes` via TSV parse. |

Additional lifts (not originally in ADR-0009):

- `mouse` actions `down` / `up` / `hold` (+ duration_ms): done.
- `mouse` modifier+click via combined EIS session: done.
- `keyboard.hold_key` (renamed to `hold_ms` arg on `keyboard.key`): done.
- xkbcommon-based typing for non-US layouts: done. Falls back to clipboard-paste when keymap can't produce the character at level 0 or 1.
- `wait` conditions `screen_change`, `element_appears`, `element_disappears`: done.
- `inspect target=focused` via walk-by-state predicate: done.
- `inspect target=window:<ref>` via window→pid → AT-SPI Application lookup: done.
- `window_query` actions `get_geometry` and `find`: done.
