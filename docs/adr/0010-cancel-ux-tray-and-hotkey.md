# ADR-0010: Cancel UX — system tray + Meta+Esc hotkey + dispatch hooks

**Status:** Accepted (2026-05-15)

Supersedes ADR-0009 §2 (cancel via SIGUSR1 only).

## Context

ADR-0009 deferred KGlobalAccel registration as a follow-up, falling back to
SIGUSR1 (`pkill -USR1 kde-mcp`). That's a power-user hatch, not a UX —
users have no visible indicator that the server is processing, and no
keyboard shortcut to abort.

The user explicitly asked for three things: visible indicator while in
flight, a hotkey to stop, and clean propagation of the cancel result
through the MCP wire.

## Decision

Three independent abort routes wired in parallel:

### 1. System tray icon (StatusNotifierItem)

Uses the `ksni 0.3` crate to register an `org.kde.StatusNotifierItem` on
the user's panel. The icon flips between themed names:
`preferences-desktop` when idle, `media-playback-start-symbolic` when a
tool is in flight. Tooltip shows the running tool's wire name.
Right-click menu has "Abort current operation" (greyed when nothing's
running) and "Quit kde-mcp".

Runs on its own OS thread with a current-thread tokio runtime so a
tray-bus disconnect can't stall the rmcp server.

### 2. KGlobalAccel Meta+Esc

The actual ADR-0009 §2 work. Registers the action via
`org.kde.kglobalaccel.doRegister` + `setShortcut`, subscribes to
`globalShortcutPressed` on `/component/kde-mcp`. KWin grabs the key at
the compositor level, so prompt-injection-induced UI can't dismiss it.

Registration is best-effort: if kglobalaccel isn't available the
fallback (SIGUSR1) still works.

### 3. SIGUSR1 fallback (unchanged)

`pkill -USR1 kde-mcp` flips the same `CancellationToken`. Useful for
testing, scripting, and for users who can't bind Meta+Esc (e.g. it's
already in use by another component).

## Dispatch integration

`KdeMcp` gains an `Arc<TrayState>` holding:
- `in_flight: AtomicUsize` — bumped by every `dispatch_audited` call
- `current_tool: Mutex<Option<String>>` — name of the most recently
  started tool (best-effort under concurrency)
- `cancel: CancellationToken` — shared with the policy gate
- `quit_requested: AtomicBool` — flipped by the tray "Quit" menu

A RAII `TrayGuard::enter()` / `Drop` wraps the awaited future in
`dispatch_audited`. Panic-safe + cancel-safe; the guard always runs.

## Consequences

- Three abort surfaces means the cancel UX degrades gracefully across
  KDE setups. ksni works on any freedesktop tray-host (most desktops,
  not just KDE). KGlobalAccel works on KDE only. SIGUSR1 works
  everywhere.
- The dispatch wrapper adds one atomic increment + a `Mutex::lock()` on
  every tool call. Sub-microsecond.
- `quit_requested` is currently only honoured by the tray's poll loop
  (the tray exits when it's set). `main()` doesn't yet poll it to
  shutdown the rmcp service. Follow-up: the tray "Quit" should also
  signal SIGTERM to the process.

## Alternatives considered

- **Borderless always-on-top overlay window** (closer to a Discord
  in-game overlay). Rejected: needs an entire GUI toolkit (Qt or
  similar) for what a tray icon already covers natively. Tray icons
  also persist across virtual desktops, which an overlay window
  wouldn't without extra work.
- **xdg-portal GlobalShortcuts** instead of KGlobalAccel. Rejected for
  the same reason ADR-0007 picked direct KWin surfaces over portals:
  the portal flow has a permission dialog and we're KDE-only by
  ADR-0002.
- **Caching the in-flight state in a `tokio::Mutex` instead of atomics**
  — rejected because the tray thread isn't in tokio, and atomic ops
  are correct for the counter-only case.
