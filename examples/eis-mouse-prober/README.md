# EIS mouse-click prober

Sibling of `eis-keystroke-prober`. Same EIS handoff sequence, but emits a single left-click at a target `(x, y)` via the `PointerAbsolute` + `Button` capabilities.

This crate is **not** a workspace member of the main `kde-mcp` crate. It lives under `examples/` as a permanent reference alongside the keystroke prober.

## What it does

1. Opens a session-bus connection to `org.kde.KWin`.
2. Calls `org.kde.KWin.EIS.RemoteDesktop.connectToEIS(2)` — `flags=2` is the **pointer-capability** bit (1=keyboard, 2=pointer, 4=touch, OR them to combine).
3. Wraps the returned FD as a `UnixStream` and constructs `ei::Context`.
4. Runs the async handshake as `ContextType::Sender`.
5. Binds Pointer + PointerAbsolute + Button + Scroll capabilities on the seat.
6. Waits for `DeviceAdded` (named `<conn-name> eis absolute device`) and `DeviceResumed`.
7. `device.start_emulating` → `pointer_abs.motion_absolute(x, y)` → `button.button(BTN_LEFT, Press)` → `button.button(BTN_LEFT, Released)` → `stop_emulating`.
8. Exits 0.

## Running

```sh
cd examples/eis-mouse-prober
cargo build --release
# Click at (400, 20) — likely a tab in a fullscreen browser:
./target/release/eis-mouse-prober 400 20
# Custom target:
./target/release/eis-mouse-prober 800 500
```

Make sure the target window is focused before running. The prober has no built-in delay; the click fires within ~100ms of process start.

## Correction to the Phase 0 spike findings

The keystroke prober's postscript at `docs/research/2026-05-14-phase-0-spikes.md` originally claimed `flags=1` was "SENDER mode" and `flags=0` was "receiver-only." That reading was wrong. The correct interpretation, found while building this mouse prober:

- `flags` is a **capability bit-mask**: 1 (keyboard) | 2 (pointer) | 4 (touch).
- Sender vs receiver is selected by `ContextType` in the libei handshake, not by this flag.
- `flags=0` advertises nothing so no `DeviceAdded` event ever fires.
- `flags=1` (keyboard) works for the keystroke prober because keyboard is what it needs; pointer caps are filtered out by KWin's `m_allowedCapabilities`.
- `flags=2` is what gets you a pointer device.
- `flags=3` (1|2) gets you both keyboard and pointer devices on one connection.

The spike-notes file has been corrected.
