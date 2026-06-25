# Phase 0 Spike Findings

Captured 2026-05-14, before Phase 0 implementation begins. Each spike was a short read-only investigation or a throwaway compile test. Plan adjustments derived from each spike are listed inline.

---

## 0.1 — rmcp `ProtocolVersion` variants

**Source:** `https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/model.rs`

`ProtocolVersion` is a `pub struct ProtocolVersion(Cow<'static, str>)` with associated constants:

```rust
pub const V_2025_11_25: Self = Self(Cow::Borrowed("2025-11-25"));
pub const V_2025_06_18: Self = Self(Cow::Borrowed("2025-06-18"));
pub const V_2025_03_26: Self = Self(Cow::Borrowed("2025-03-26"));
pub const V_2024_11_05: Self = Self(Cow::Borrowed("2024-11-05"));
pub const LATEST: Self = Self::V_2025_11_25;
```

**Decision:** use `ProtocolVersion::V_2025_11_25`. The spec citation (`MCP 2025-11-25` per §13.1) matches what rmcp 1.7 advertises. Task 11's `with_protocol_version(...)` call should pass `ProtocolVersion::V_2025_11_25`. **No Task 14 spec correction needed.**

---

## 0.2 — `#[tool_router]` composition across `impl` blocks

**Test:** spun up a throwaway crate with two `#[tool_router] impl Foo { ... }` blocks on the same type, one `#[tool]`-annotated method in each. Ran `cargo check`.

**Result — failure (expected per the rmcp design):**

```
error[E0592]: duplicate definitions with name `tool_router`
  --> src/main.rs:22:1
   |
22 | #[tool_router]
   | ^^^^^^^^^^^^^^ duplicate definitions for `tool_router`
...
30 | #[tool_router]
   | -------------- other definition for `tool_router`
```

The proc-macro emits a `tool_router()` associated function inside each annotated `impl` block; two annotated blocks → two definitions of the same fn → E0592.

**Verification of the single-block path:** collapsing both methods into one `#[tool_router] impl Foo { ... }` block compiles cleanly (one dead-code warning that resolves once the struct is wired to `rmcp::serve(...)`).

**Decision:** **Task 11 must use a single `#[tool_router] impl KdeMcp { ... }` block** with all 16 wire-named `#[tool]` methods inside it. Comment dividers ("// ---- Observation tools ----", etc.) preserve the four-category structure visually, but the macro annotation is applied exactly once.

The plan's "if Spike 0.2 fails" branch (option 2 from the rust-stack research note) is the path we take.

### Sub-finding (not in the original spike list) — `schemars` version

`rmcp 1.7` depends on `schemars 1.x` internally. The plan's Cargo.toml pins `schemars = "0.8"`, which produces:

```
note: there are multiple different versions of crate `schemars` in the dependency graph
   --> .../schemars-1.2.1/src/lib.rs (expected)
   --> .../schemars-0.8.22/src/lib.rs (found)
```

Every `Parameters<MyArgs>` would fail to satisfy `rmcp::schemars::JsonSchema`.

**Decision:** **Task 1 Cargo.toml must pin `schemars = "1.0"`** (the current major matching rmcp's internal). The macro shape (`#[derive(schemars::JsonSchema)]`) is unchanged across 0.8 → 1.x for our uses; we just need the same major as rmcp.

---

## 0.3 — `atspi-proxies::action::Action` interface

**Source:** `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi-proxies/src/action.rs`

The trait is named **`Action`** (decorated with `#[zbus::proxy(...)]`; the generated proxy types are `ActionProxy<'_>` and `ActionProxyBlocking<'_>`). All methods return `zbus::Result<T>`:

| Method | Signature |
|---|---|
| `do_action` | `fn do_action(&self, index: i32) -> zbus::Result<bool>` |
| `get_actions` | `fn get_actions(&self) -> zbus::Result<Vec<Action>>` |
| `get_description` | `fn get_description(&self, index: i32) -> zbus::Result<String>` |
| `get_key_binding` | `fn get_key_binding(&self, index: i32) -> zbus::Result<String>` |
| `get_localized_name` | `fn get_localized_name(&self, index: i32) -> zbus::Result<String>` |
| `get_name` | `fn get_name(&self, index: i32) -> zbus::Result<String>` |
| `n_actions` | `fn n_actions(&self) -> zbus::Result<i32>` (D-Bus property) |

**Spec drift to note in the Phase 1 plan:** the rust-stack research note guessed at `get_action_name` / `get_action_description`. The actual names are `get_name` / `get_description`. The implementation of `act_on_element` (Phase 1, not Phase 0) should use the verified names. No Phase 0 code calls these.

---

## 0.4 — `reis` motion / button / frame methods

**Source:** `https://raw.githubusercontent.com/ids1024/reis/master/src/eiproto_ei.rs` (the auto-generated sender-side protocol module — reis does not have per-device files; everything sender-related lives in this one generated file).

| Method | Module | Signature |
|---|---|---|
| `motion_relative` | `pointer` | `pub fn motion_relative(&self, x: f32, y: f32)` |
| `motion_absolute` | `pointer_absolute` | `pub fn motion_absolute(&self, x: f32, y: f32)` |
| `button` | `button` | `pub fn button(&self, button: u32, state: super::button::ButtonState)` |
| `frame` | `device` | `pub fn frame(&self, last_serial: u32, timestamp: u64)` |

All return `()`. Coordinates are `f32` logical pixels. Frame's `timestamp` is microseconds; `last_serial` is the serial of the last protocol event (the demo examples use 0 for self-driven frames at session start).

**Note:** the rust-stack research note speculated about `motion_absolute(x, y)` signature — confirmed exactly. The `xkbcommon` integration for keysym → evdev-minus-8 keycode mapping is still required for Phase 2 keyboard injection.

---

## 0.5 — KWin `org.kde.KWin.EIS.RemoteDesktop` D-Bus signature

**Source:** live `busctl --user introspect` against the dev machine's KDE Plasma 6 Wayland session (bus name `org.kde.KWin`).

The interface is exposed at the **child object path** `/org/kde/KWin/EIS/RemoteDesktop` (not at `/org/kde/KWin/EIS` — that path has only Introspectable/Peer/Properties). Tree under `/org/kde/KWin/EIS`:

```
/org/kde/KWin/EIS
├── /org/kde/KWin/EIS/InputCapture       (receiver-side; not relevant)
└── /org/kde/KWin/EIS/RemoteDesktop       (sender-side; this one)
```

Interface signature:

```
org.kde.KWin.EIS.RemoteDesktop  interface
  .connectToEIS    method    i -> hi
  .disconnect      method    i -> -
```

Reading the D-Bus type signatures:

- **`connectToEIS`**: input `i` (one `int32`), output `hi` (one `UNIX_FD` + one `int32`). The `h` is the libei socket FD to hand to `reis::ei::Context::new(UnixStream::from(fd))`. The trailing `i` is almost certainly the connection ID required by `disconnect`.
- **`disconnect`**: input `i` (the connection ID from `connectToEIS`), no output.

**What the input `i` to `connectToEIS` is**: not documented in the introspect output. KDE's upstream source (per `kwin_wayland/src/plugins/eis/eis_backend.cpp` historically) accepts a flag/version int. Pass `0` initially; revisit if Phase 2 surfaces errors.

**Decision for `src/kwin/eis.rs` (Phase 1/2):** the proxy macro should declare:

```rust
#[zbus::proxy(
    interface = "org.kde.KWin.EIS.RemoteDesktop",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/EIS/RemoteDesktop"
)]
trait RemoteDesktop {
    fn connect_to_eis(&self, flags: i32) -> zbus::Result<(zvariant::OwnedFd, i32)>;
    fn disconnect(&self, connection_id: i32) -> zbus::Result<()>;
}
```

(zbus's snake_case rename: `connect_to_eis` is the Rust name; the wire method is `connectToEIS`.)

Phase 0 does not call this. Spec §13.1's Phase 0 spike checklist is satisfied by recording the signature here.

---

## Summary of plan adjustments

| Source | Adjustment |
|---|---|
| Spike 0.1 | No change. Use `ProtocolVersion::V_2025_11_25`. |
| Spike 0.2 | Task 11: **single `#[tool_router] impl KdeMcp { ... }` block** with all 16 `#[tool]` methods. Comment dividers preserve grouping. |
| Spike 0.2 sub-finding | Task 1 Cargo.toml: **`schemars = "1.0"`** (was `"0.8"`). |
| Spike 0.3 | Note `get_name` / `get_description` correction for Phase 1's `act_on_element` plan. No Phase 0 impact. |
| Spike 0.4 | Confirms plan §13.1 expectation; `xkbcommon` still required for Phase 2. No Phase 0 impact. |
| Spike 0.5 | KWin EIS proxy declaration template recorded above. No Phase 0 impact. |

These adjustments are applied in-flight during Tasks 1 and 11; the plan file is not edited (deltas are recorded here and in commit messages).

---

## Postscript — live EIS keystroke prober run (2026-05-14)

During the Phase 0 end-of-task POC pass we tested the API calls end-to-end to confirm the future job is doable. After verifying the MCP wire end-to-end (initialise → tools/list → tools/call against the live binary, all 16 tool names + schemas + hints round-tripping correctly), we verified the libei keystroke path — the riskiest unverified primitive across all remaining phases.

A standalone prober crate was written at `examples/eis-keystroke-prober/` (committed to the repo as a permanent reference; not a workspace member of kde-mcp). It exercises the exact code path Phase 2's `keyboard.key` will use: zbus → `connectToEIS` → FD → `ei::Context::new` → `handshake_tokio(Sender)` → bind capabilities → wait for `DeviceAdded` + `DeviceResumed` → `start_emulating` → `Keyboard::key` + `Device::frame` → `stop_emulating`.

### What worked

End-to-end on the dev machine (CachyOS, KWin 6.x Wayland):

1. `connectToEIS(1)` returned `(OwnedFd, connection_id)`. Handshake completed.
2. `SeatAdded { name: "kde-mcp-eis-prober seat" }` fired; `bind_capabilities(Keyboard | Pointer | PointerAbsolute | Button | Scroll)` accepted.
3. `DeviceAdded { name: "kde-mcp-eis-prober eis keyboard", type: Virtual, has_kb: true }` materialised the virtual keyboard.
4. `DeviceResumed { serial: 2 }` signalled "ready for input emulation."
5. `device.start_emulating(last_serial=2, sequence=0)`, then four key frames (Ctrl press / Tab press / Tab release / Ctrl release), then `stop_emulating`.
6. Prober exited 0. The keystroke arrived at Brave (confirmed visually — pre-step: a KWin `workspace.activeWindow = brave_window` ran to focus Brave on Desktop 1).

### Findings that update the Phase 2 plan (and worth noting in the spec)

| Finding | Implication |
|---|---|
| **`flags` is a CAPABILITY BIT-MASK**, not a mode bit. Per KWin's `eisbackend.cpp`: `1 = keyboard`, `2 = pointer (incl. PointerAbsolute + Button + Scroll)`, `4 = touch`. The keystroke prober's `flags = 1` works because it happens to include keyboard. The Phase 2 keyboard tool uses `1`; the mouse tool uses `2`; combine for both. `flags = 0` advertises nothing → no device gets added. **Correction noted 2026-05-14 while building `examples/eis-mouse-prober/`; the original "SENDER mode" reading of `flags=1` was wrong** — sender vs receiver is selected by `ContextType` in the handshake, not by this flag. | Phase 2's `src/kwin/eis.rs` should use named constants: `const KWIN_EIS_CAP_KEYBOARD: i32 = 1;`, `const KWIN_EIS_CAP_POINTER: i32 = 2;`, `const KWIN_EIS_CAP_TOUCH: i32 = 4;`. Pass the bitwise OR of what the tool needs. |
| **Return type is the struct `(h, i)`, not two separate out-args.** D-Bus introspection notation was misleading; zbus correctly typechecks it as `(OwnedFd, i32)`. The `i32` is the connection id passed back to `disconnect`. | Proxy signature: `fn connect_to_eis(&self, flags: i32) -> zbus::Result<(zvariant::OwnedFd, i32)>`. Disconnect: `fn disconnect(&self, connection_id: i32) -> zbus::Result<()>`. |
| **No `.desktop` / `X-KDE-DBUS-Restricted-Interfaces` registration was needed** for sender mode on this machine. The spec assumed (correctly, for `ScreenShot2`) the trader-check applied here too; it didn't. | Phase 0's smoke procedure already noted the `.desktop` install for ScreenShot2. EIS apparently doesn't go through `KApplicationTrader`. Phase 1 may discover this needs revisiting for sandboxed apps; document then. |
| **`reis 0.6.1`'s tokio high-level API is sufficient.** `handshake_tokio` + `EiEventStream` + `match EiEvent::{SeatAdded, DeviceAdded, DeviceResumed}` covers the bring-up. Protocol methods are on `device.device()` (returns `&ei::Device`); the keyboard handle is `device.interface::<ei::Keyboard>()`. No need to drop to the low-level `context.read()` / `context.pending_event()` loop that `reis::examples::type-text.rs` uses. | Phase 2's `src/kwin/eis.rs` can stay at the high-level API. Simpler, fewer state-machine bugs. |
| **`device.start_emulating(last_serial, sequence)` parameter order matters** despite `type-text.rs` calling it `(sequence, last_serial)`. Per the rustdoc signature, `last_serial` is the first parameter. The example works either way because both are `u32` and KWin appears tolerant; for correctness, follow the signature. | Phase 2 implementations should pass `(last_serial, sequence)` and not crib the parameter order from `type-text.rs` verbatim. |
| **Keycodes are evdev raw** (Ctrl = 29, Tab = 15) when read from `linux/input-event-codes.h`. The `keycode - 8` form in `type-text.rs` exists because that example starts from xkb keycodes (= evdev + 8) via `xkbcommon`. | Phase 2's keymap layer must subtract 8 if it sources keycodes from `xkbcommon::Keymap`; nothing to subtract if it hard-codes evdev codes for known keys (e.g. arrow keys, modifiers, function keys). |
| **Spec §13.1's Cargo deps need one more entry** for Phase 2: `futures-util = "0.3"`. The reis `EiEventStream` impls `futures::Stream` — to call `.next().await` on it you need `futures_util::StreamExt` in scope. | Add to §13.1 when Phase 2 starts; or wait until Phase 2's plan adds it. Either is fine; this postscript is the canonical reference. |

### Reference crate left in-tree

`examples/eis-keystroke-prober/` is a complete, standalone proof. It is intentionally not a workspace member of `kde-mcp` (its `reis` + `futures-util` dependencies are Phase 2 deps that Phase 0 doesn't need). To re-run the proof on a different machine or after a KDE update:

```sh
cd examples/eis-keystroke-prober
cargo run --release
```

…with the target window focused. See `examples/eis-keystroke-prober/README.md`.
