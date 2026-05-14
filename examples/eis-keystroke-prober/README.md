# EIS keystroke prober

Standalone proof-of-concept that verifies libei-based keystroke injection works through KWin's direct `org.kde.KWin.EIS.RemoteDesktop` interface — the exact code path Phase 2 of `kde-mcp` will use for the `keyboard.key` and `keyboard.type` tools.

This crate is **not** a workspace member of the main `kde-mcp` crate. It lives under `examples/` as a permanent reference. Building it pulls in `reis` and `futures-util`, which Phase 0 of `kde-mcp` deliberately omits.

## What it does

1. Opens a session-bus connection to `org.kde.KWin`.
2. Calls `org.kde.KWin.EIS.RemoteDesktop.connectToEIS(1)` — `flags=1` is the **SENDER** mode bit; `flags=0` opens a receiver-only connection on which no virtual devices are ever created.
3. Wraps the returned FD as a `std::os::unix::net::UnixStream` and constructs an `ei::Context`.
4. Runs the async handshake as `ContextType::Sender` via `reis::ei::Context::handshake_tokio`.
5. Drives the `EiEventStream`: on `SeatAdded` binds Keyboard/Pointer/Button/Scroll/PointerAbsolute capabilities; on `DeviceAdded` saves the keyboard-capable device; on `DeviceResumed` records the serial and breaks the loop.
6. Calls `device.start_emulating(last_serial, sequence=0)`, then four key frames (Ctrl press, Tab press, Tab release, Ctrl release), then `stop_emulating`.
7. Exits 0.

## Running

```sh
# From the repo root:
cd examples/eis-keystroke-prober
cargo build --release

# Focus the target window (a browser, terminal, anything keyboard-accepting),
# then within ~5 seconds run:
./target/release/eis-keystroke-prober
```

The prober emits one Ctrl+Tab. If the focused window is a browser with more than one tab open, the active tab changes. Otherwise the keystroke is delivered but has no visible effect.

## Findings worth knowing (recorded 2026-05-14)

- The D-Bus introspection signature for `connectToEIS` is `i → hi`, which actually means **one input `i32`, one output struct `(OwnedFd, i32)`** — not two separate out-arguments. The trailing `i32` is the connection id passed back to `disconnect`.
- `flags=1` enables sender mode. KWin's introspection doesn't document the flag bits; this was verified empirically.
- On a personal-use KDE Plasma 6 / Wayland session, no `.desktop`-file `X-KDE-DBUS-Restricted-Interfaces` setup was needed. (The `ScreenShot2` interface in the same KWin process does require that; EIS apparently does not.)
- Keycodes are evdev raw (Ctrl=29, Tab=15). The `keycode - 8` form in reis's `type-text.rs` example exists because that example starts from X11/xkb keycodes (= evdev + 8). When the source of keycodes is `/usr/include/linux/input-event-codes.h`, no subtraction is needed.
- `device.start_emulating(last_serial, sequence)` — order is `last_serial` first. reis's `type-text.rs` example calls it `(sequence, last_serial)`, which appears to work but disagrees with the signature.

See `docs/research/2026-05-14-phase-0-spikes.md` for the full spike record.
