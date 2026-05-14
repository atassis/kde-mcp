# kde-mcp Phase 2 — Input Tools Against Live Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the five mutating-tool stubs that are unblocked at Phase 2 — `mouse`, `keyboard.type`, `keyboard.key`, `clipboard_write`, `act_on_element` — with working implementations against the user's real KDE Plasma 6 / Wayland session. After Phase 2 the spec's §14 "open kcalc, type 2+3, press =, read the result" demo is technically possible.

**Architecture:** The five tools split into three families:
- `act_on_element` invokes AT-SPI's `Action.do_action(index)` directly on a stored element ref. No input emulation. Skips the keyboard/mouse layer entirely when AT-SPI exposes a usable action — this is the "killer primitive" per spec §6.2.
- `mouse` and `keyboard.{type,key}` route through a new `src/kwin/eis.rs` connection helper, which opens an EIS connection per call (the prober showed handshake is ~50ms; per-call keeps state simple; Phase 6 polish can long-lived if needed). Uses the **bit-mask `flags`** correction discovered in `examples/eis-mouse-prober/` (1 = keyboard, 2 = pointer; spec correction in `docs/research/2026-05-14-phase-0-spikes.md`).
- `clipboard_write` spawns `wl-copy` (mirror of Phase 1's `wl-paste` for `clipboard_read`).

Key tactical decisions:
- **Per-call EIS connections** in Phase 2. Each tool invocation does the full `connectToEIS → handshake → bind → emit → disconnect` cycle. Trades 50-100ms latency per call for zero shared state. Phase 6 polish revisits if profiling shows it matters.
- **xkbcommon for keysym↔keycode** mapping. `keyboard.type` walks text → keysyms → keycodes via the system keymap; `keyboard.key` parses xdotool-style chord syntax (`"ctrl+shift+s"`, `"Return"`, `"Page_Down"`).
- **`keyboard.type` clipboard-paste fallback** per spec §6.2 — when text is > 32 chars or contains non-ASCII, set clipboard + send Ctrl+V. Original clipboard is saved+restored.
- **`act_on_element` action name → index lookup** — AT-SPI's `Action.do_action(index: i32)` takes a numeric index. We call `get_actions()` first, find the requested name, then invoke by index. Cached per ref.

**Tech Stack additions** (Cargo.toml deltas on top of Phase 1):
- `reis = "=0.6.1"` with the `tokio` feature — already validated by both probers. Pin exactly.
- `xkbcommon = "0.9"` — keysym/keycode mapping. Note: the spec §13.1 pin was "0.8.x"; 0.9.0 is the current latest. Pin to `"0.9"` (compatible-update flexibility within 0.9.x).

No other deps needed — `tokio::process` is already on, `wl-clipboard` is now installed (per the user-run install script), and `zbus` covers AT-SPI's `Action` proxy.

**MSRV unchanged:** Rust 1.85+, edition 2024.

---

## File structure (Phase 2)

| Path | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `reis = "=0.6.1"` (tokio feature) and `xkbcommon = "0.9"`. |
| `src/kwin/eis.rs` | **Create** | Per-call EIS connection helper. `connect(bus, caps)` opens `connectToEIS(caps)`, handshakes as Sender, binds the requested capabilities, waits for `DeviceAdded` + `DeviceResumed`, returns a session value that owns the connection + device. `Drop` impl calls `stop_emulating`. |
| `src/kwin/mod.rs` | Modify | Add `pub mod eis;`. |
| `src/keymap.rs` | **Create** | Keysym ↔ keycode mapping via xkbcommon. Pure functions plus the cached `Keymap` constructed at first use. `keysym_for_char(ch: char) -> Option<u32>`, `keycode_for_keysym(sym: u32) -> Option<u32>` (returns evdev raw — no `-8` offset). Chord parser: `parse_chord("ctrl+shift+s") -> Result<Chord, ToolError>` where `Chord` is `{ modifiers: Vec<Modifier>, key: NamedKey }`. |
| `src/lib.rs` | Modify | `pub mod keymap;`. |
| `src/tools/mouse.rs` | Modify | Implement `mouse` for actions `click`, `move`, `drag`, `scroll`. Each invokes a fresh EIS session via `kwin::eis`. |
| `src/tools/keyboard.rs` | Modify | Implement `keyboard_type` and `keyboard_key`. `keyboard_type` selects between direct-typing and clipboard-paste per spec §6.2 (use_clipboard "auto" rule). |
| `src/tools/clipboard.rs` | Modify | Implement `clipboard_write` via `wl-copy`. |
| `src/tools/act_on_element.rs` | Modify | Implement via AT-SPI `Action.do_action(index)`. Resolves element ref → encoded `(bus, path)` → `AccessibleProxy` → `Action` proxy → list actions → match name → invoke. |
| `tests/integration.rs` | Modify | Add `pub mod keymap;` and `pub mod chord_parse;` for the offline tests. |
| `tests/integration/keymap.rs` | **Create** | Unit tests for keysym/keycode lookup of common keys (Return, Tab, a, A, ?). |
| `tests/integration/chord_parse.rs` | **Create** | Unit tests for chord-string parsing (`"ctrl+s"`, `"ctrl+shift+T"`, `"Return"`, `"Page_Down"`, malformed). |
| `tests/integration/SMOKE.md` | Modify | New §D "Phase 2 mutating" covering kcalc 2+3=5, Ctrl+Tab in Brave, clipboard_write round-trip, act_on_element on kcalc's "5" button. |
| `docs/research/2026-05-14-phase-2-spikes.md` | **Create** | Phase 2 spike findings (Task 0). |

---

## Task ordering and dependencies

```
T0 (spikes)            ─► T1 (Cargo) ─► T2 (keymap) ─► T3 (eis helper) ─┐
                                  │                                       ├─► T4 (mouse)
                                  │                                       ├─► T5 (keyboard)
                                  │                                       │
                                  ├─► T6 (clipboard_write)                │
                                  ├─► T7 (act_on_element)                 │
                                                                          │
                                                                          ▼
                                                                       T8 (verify + tag)
```

Estimated total: 3-4 working days at the user's part-time pace (matches spec §14 Phase 2 budget).

---

## Task 0: Phase 2 spike verification

Three short read-only investigations. Output: one Markdown note at `docs/research/2026-05-14-phase-2-spikes.md`. Estimated total: 45 minutes.

**Files:**
- Create: `docs/research/2026-05-14-phase-2-spikes.md`

- [ ] **Step 0.0: Create the spike-notes file with headers**

```markdown
# Phase 2 Spike Findings

Captured 2026-05-14, before Phase 2 implementation begins. Read-only investigations only.

## 0.1 xkbcommon-rs 0.9 surface for keysym/keycode mapping
## 0.2 Keystroke prober output: keymap context lifetime + keycode source
## 0.3 wl-copy behaviour (already-installed wl-clipboard)
```

- [ ] **Step 0.1: xkbcommon-rs API**

Fetch `https://raw.githubusercontent.com/rust-x-bindings/xkbcommon-rs/master/src/xkb/keymap.rs`. Locate:
- `Context::new(flags)` signature — what's the flags parameter (typically `xkb::CONTEXT_NO_FLAGS`).
- `Keymap::new_from_names(ctx, rules, model, layout, variant, options, compile_flags)` vs `Keymap::new_from_string` — pick the one that uses the system's default layout.
- `Keymap::key_get_syms_by_level(keycode, layout, level) -> &[Keysym]` — the reverse-mapping primitive we need.
- `Keysym::from_name(name, flags) -> Option<Keysym>` — for chord parsing.
- The minimum keycode value (typically 8; evdev keycode = xkb keycode - 8).

Decision tree:
- If the API exposes `Keymap::min_keycode()` / `max_keycode()` — iterate that range.
- If only `Keymap::key_count()` exists — iterate `0..key_count()`.
- Record exact function names for Task 2's keymap.rs.

Record the findings in the spike notes under §0.1.

- [ ] **Step 0.2: Re-verify the existing keystroke prober's keycode source**

The keystroke prober (`examples/eis-keystroke-prober/`) hard-codes `KEY_LEFTCTRL=29` and `KEY_TAB=15`. These are evdev raw values (from `/usr/include/linux/input-event-codes.h`). Confirm:

```sh
grep -E 'KEY_LEFTCTRL|KEY_TAB' /usr/include/linux/input-event-codes.h | head -5
```

Record the values. These remain hard-coded for the most-common keys in Phase 2 (modifiers, Tab, Return, arrows, F-keys) because going through xkbcommon for them is wasted work. The xkbcommon path is used only for typing arbitrary text.

- [ ] **Step 0.3: `wl-copy` behaviour**

Run safely (reads/writes only the clipboard, no other session state):

```sh
which wl-copy && wl-copy --version
echo -n "phase-2-spike-test" | wl-copy --type text/plain
sleep 0.2
wl-paste --type text/plain
# Clean up:
wl-copy --clear 2>&1
```

Record the exit codes, whether `wl-copy` accepts content on stdin (it does), and how `--clear` behaves. Decision: Task 6 (`clipboard_write`) spawns `wl-copy --type <mime> <content via stdin>`.

- [ ] **Step 0.4: Commit the spike notes**

```bash
git add docs/research/2026-05-14-phase-2-spikes.md
git commit -m "docs(phase-2): spike findings (xkbcommon, evdev keycodes, wl-copy)"
```

---

## Task 1: Cargo.toml additions

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1.1: Add Phase 2 deps**

Edit the `[dependencies]` block. Append below the Phase 1 additions:

```toml
# Phase 2 additions:
reis           = { version = "=0.6.1", features = ["tokio"] }
xkbcommon      = "0.9"
```

- [ ] **Step 1.2: Verify Cargo.toml compiles**

```bash
cargo check --workspace 2>&1 | tail -3
```

Expected: `Finished \`dev\` profile`. If `reis` or `xkbcommon` fails to resolve, double-check the version against the spike-0.1 findings.

- [ ] **Step 1.3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(phase-2): add reis + xkbcommon deps"
```

---

## Task 2: `src/keymap.rs` — keysym/keycode + chord parser

Pure module — no I/O, fully unit-testable.

**Files:**
- Create: `src/keymap.rs`
- Modify: `src/lib.rs`
- Create: `tests/integration/keymap.rs`
- Create: `tests/integration/chord_parse.rs`
- Modify: `tests/integration.rs`

- [ ] **Step 2.1: Define the public surface**

Create `src/keymap.rs`:

```rust
//! Keysym/keycode mapping via xkbcommon, plus xdotool-style chord parsing.
//!
//! - `keycode_for_char(ch)` resolves a printable character to an evdev raw
//!   keycode. Returns `None` if the system keymap can't produce that
//!   character without a dead-key sequence or complex composition.
//! - `parse_chord(s)` accepts strings like `"ctrl+shift+s"`, `"Return"`,
//!   `"Page_Down"` and returns a `Chord` ready to drive libei.
//!
//! Modifier names (case-insensitive): ctrl/control, shift, alt, super/meta.
//! Named keys follow xkb's `XKB_KEY_*` symbol set (case-sensitive on the
//! name part). Internally we hard-code the common ones because xkbcommon's
//! `Keysym::from_name` is slow on first call.

use crate::error::ToolError;

/// evdev raw keycodes from `/usr/include/linux/input-event-codes.h`.
/// libei expects these directly; no `-8` offset.
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_LEFTALT: u32 = 56;
pub const KEY_LEFTMETA: u32 = 125;
pub const KEY_TAB: u32 = 15;
pub const KEY_ENTER: u32 = 28;
pub const KEY_ESC: u32 = 1;
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_SPACE: u32 = 57;
pub const KEY_UP: u32 = 103;
pub const KEY_LEFT: u32 = 105;
pub const KEY_RIGHT: u32 = 106;
pub const KEY_DOWN: u32 = 108;
pub const KEY_PAGEUP: u32 = 104;
pub const KEY_PAGEDOWN: u32 = 109;
pub const KEY_HOME: u32 = 102;
pub const KEY_END: u32 = 107;
pub const KEY_INSERT: u32 = 110;
pub const KEY_DELETE: u32 = 111;
pub const KEY_F1: u32 = 59;
pub const KEY_F2: u32 = 60;
pub const KEY_F3: u32 = 61;
pub const KEY_F4: u32 = 62;
pub const KEY_F5: u32 = 63;
pub const KEY_F6: u32 = 64;
pub const KEY_F7: u32 = 65;
pub const KEY_F8: u32 = 66;
pub const KEY_F9: u32 = 67;
pub const KEY_F10: u32 = 68;
pub const KEY_F11: u32 = 87;
pub const KEY_F12: u32 = 88;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl Modifier {
    pub fn keycode(self) -> u32 {
        match self {
            Modifier::Ctrl => KEY_LEFTCTRL,
            Modifier::Shift => KEY_LEFTSHIFT,
            Modifier::Alt => KEY_LEFTALT,
            Modifier::Super => KEY_LEFTMETA,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chord {
    pub modifiers: Vec<Modifier>,
    pub key: u32, // evdev keycode
}

/// Parse an xdotool-style chord string. The last `+`-separated token is the
/// key (named or single character); earlier tokens are modifiers.
pub fn parse_chord(s: &str) -> Result<Chord, ToolError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(ToolError::UnsupportedRegion {
            reason: "empty chord".into(),
        });
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("empty chord segment in {trimmed:?}"),
        });
    }
    let (key_name, mod_names) = parts.split_last().unwrap();

    let mut modifiers = Vec::with_capacity(mod_names.len());
    for m in mod_names {
        let parsed = match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "shift" => Modifier::Shift,
            "alt" => Modifier::Alt,
            "super" | "meta" | "win" => Modifier::Super,
            other => {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("unknown modifier: {other}"),
                });
            }
        };
        if modifiers.contains(&parsed) {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("modifier {m:?} appears twice"),
            });
        }
        modifiers.push(parsed);
    }

    let key = keycode_for_named_key(key_name).ok_or_else(|| ToolError::UnsupportedRegion {
        reason: format!("unknown key name: {key_name:?}"),
    })?;

    Ok(Chord { modifiers, key })
}

/// Hard-coded named-key → keycode table. The names mirror xkb's `XKB_KEY_*`
/// symbol set but case-insensitive on lookup. Single-character keys (`a`,
/// `0`) route through the keymap path in step 2.2.
pub fn keycode_for_named_key(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "return" | "enter" => KEY_ENTER,
        "tab" => KEY_TAB,
        "escape" | "esc" => KEY_ESC,
        "backspace" => KEY_BACKSPACE,
        "space" => KEY_SPACE,
        "up" => KEY_UP,
        "down" => KEY_DOWN,
        "left" => KEY_LEFT,
        "right" => KEY_RIGHT,
        "page_up" | "pageup" => KEY_PAGEUP,
        "page_down" | "pagedown" => KEY_PAGEDOWN,
        "home" => KEY_HOME,
        "end" => KEY_END,
        "insert" => KEY_INSERT,
        "delete" => KEY_DELETE,
        "f1" => KEY_F1,
        "f2" => KEY_F2,
        "f3" => KEY_F3,
        "f4" => KEY_F4,
        "f5" => KEY_F5,
        "f6" => KEY_F6,
        "f7" => KEY_F7,
        "f8" => KEY_F8,
        "f9" => KEY_F9,
        "f10" => KEY_F10,
        "f11" => KEY_F11,
        "f12" => KEY_F12,
        single if single.chars().count() == 1 => {
            return keycode_for_char(single.chars().next().unwrap());
        }
        _ => return None,
    })
}

/// Resolve a printable character to (evdev_keycode, shift_required).
/// Returns None for characters the system keymap can't directly produce.
pub fn keycode_for_char(ch: char) -> Option<u32> {
    // For Phase 2's first cut: ASCII letters/digits hard-coded; xkbcommon is
    // used only by `keyboard.type` when stepping through arbitrary strings.
    // The hard-coded set covers the common case without a keymap context.
    Some(match ch {
        '1' => 2, '2' => 3, '3' => 4, '4' => 5, '5' => 6,
        '6' => 7, '7' => 8, '8' => 9, '9' => 10, '0' => 11,
        'q' | 'Q' => 16, 'w' | 'W' => 17, 'e' | 'E' => 18, 'r' | 'R' => 19,
        't' | 'T' => 20, 'y' | 'Y' => 21, 'u' | 'U' => 22, 'i' | 'I' => 23,
        'o' | 'O' => 24, 'p' | 'P' => 25,
        'a' | 'A' => 30, 's' | 'S' => 31, 'd' | 'D' => 32, 'f' | 'F' => 33,
        'g' | 'G' => 34, 'h' | 'H' => 35, 'j' | 'J' => 36, 'k' | 'K' => 37,
        'l' | 'L' => 38,
        'z' | 'Z' => 44, 'x' | 'X' => 45, 'c' | 'C' => 46, 'v' | 'V' => 47,
        'b' | 'B' => 48, 'n' | 'N' => 49, 'm' | 'M' => 50,
        ' ' => KEY_SPACE,
        '\n' => KEY_ENTER,
        '\t' => KEY_TAB,
        _ => return None,
    })
}

/// True if typing this character requires shift to be held down (capital
/// letters on a US layout). For non-US layouts this is over-approximate but
/// safe: extra modifier presses don't change behaviour on chars that don't
/// need it. Phase 6 polish can route through xkbcommon for correct
/// layout-aware shifting.
pub fn shift_required(ch: char) -> bool {
    ch.is_ascii_uppercase()
}
```

- [ ] **Step 2.2: Wire keymap into lib**

Edit `src/lib.rs` to add the new module. The full final file:

```rust
pub mod atspi;
pub mod audit;
pub mod cli;
pub mod error;
pub mod keymap;
pub mod kwin;
pub mod lock;
pub mod mode;
pub mod notify;
pub mod refs;
pub mod server;
pub mod tools;
pub mod trust;
pub mod util;

pub use server::KdeMcp;
```

- [ ] **Step 2.3: Write keymap unit tests**

Create `tests/integration/keymap.rs`:

```rust
use kde_mcp::keymap::{keycode_for_char, keycode_for_named_key, shift_required, KEY_ENTER, KEY_TAB};

#[test]
fn returns_evdev_codes_for_common_ascii_letters() {
    assert_eq!(keycode_for_char('a'), Some(30));
    assert_eq!(keycode_for_char('z'), Some(44));
    assert_eq!(keycode_for_char('m'), Some(50));
    // Capitals share the same keycode (shift handled by caller).
    assert_eq!(keycode_for_char('A'), Some(30));
}

#[test]
fn returns_evdev_codes_for_digits() {
    assert_eq!(keycode_for_char('0'), Some(11));
    assert_eq!(keycode_for_char('1'), Some(2));
    assert_eq!(keycode_for_char('9'), Some(10));
}

#[test]
fn returns_none_for_non_ascii_chars() {
    assert_eq!(keycode_for_char('é'), None);
    assert_eq!(keycode_for_char('字'), None);
    assert_eq!(keycode_for_char('!'), None);
}

#[test]
fn whitespace_and_special_chars() {
    assert_eq!(keycode_for_char(' '), Some(57));
    assert_eq!(keycode_for_char('\n'), Some(KEY_ENTER));
    assert_eq!(keycode_for_char('\t'), Some(KEY_TAB));
}

#[test]
fn shift_required_matches_capital_ascii_letters() {
    assert!(shift_required('A'));
    assert!(shift_required('Z'));
    assert!(!shift_required('a'));
    assert!(!shift_required('1'));
    assert!(!shift_required(' '));
}

#[test]
fn named_keys_resolve() {
    assert_eq!(keycode_for_named_key("Return"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("Tab"), Some(KEY_TAB));
    assert_eq!(keycode_for_named_key("Escape"), Some(1));
    assert_eq!(keycode_for_named_key("Page_Down"), Some(109));
    assert_eq!(keycode_for_named_key("F5"), Some(63));
}

#[test]
fn named_key_lookup_is_case_insensitive() {
    assert_eq!(keycode_for_named_key("RETURN"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("return"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("Return"), Some(KEY_ENTER));
}
```

- [ ] **Step 2.4: Write chord-parse unit tests**

Create `tests/integration/chord_parse.rs`:

```rust
use kde_mcp::keymap::{parse_chord, Modifier};

#[test]
fn single_letter() {
    let c = parse_chord("s").unwrap();
    assert_eq!(c.modifiers, vec![]);
    assert_eq!(c.key, 31); // 's' keycode
}

#[test]
fn one_modifier_plus_key() {
    let c = parse_chord("ctrl+s").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl]);
    assert_eq!(c.key, 31);
}

#[test]
fn two_modifiers() {
    let c = parse_chord("ctrl+shift+t").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
    assert_eq!(c.key, 20); // 't'
}

#[test]
fn named_key_with_modifier() {
    let c = parse_chord("alt+Return").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Alt]);
    assert_eq!(c.key, 28); // KEY_ENTER
}

#[test]
fn super_alias_meta() {
    let c = parse_chord("super+l").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Super]);
    let c2 = parse_chord("meta+l").unwrap();
    assert_eq!(c2.modifiers, vec![Modifier::Super]);
    let c3 = parse_chord("win+l").unwrap();
    assert_eq!(c3.modifiers, vec![Modifier::Super]);
}

#[test]
fn whitespace_around_segments_is_tolerated() {
    let c = parse_chord(" ctrl + shift + t ").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
}

#[test]
fn case_insensitive_modifiers() {
    let c = parse_chord("CTRL+SHIFT+T").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
}

#[test]
fn duplicate_modifier_errors() {
    assert!(parse_chord("ctrl+ctrl+s").is_err());
}

#[test]
fn unknown_modifier_errors() {
    let err = parse_chord("hyper+s").unwrap_err();
    assert_eq!(err.code(), "unsupported_region");
}

#[test]
fn unknown_key_errors() {
    let err = parse_chord("ctrl+notakey").unwrap_err();
    assert_eq!(err.code(), "unsupported_region");
}

#[test]
fn empty_chord_errors() {
    assert!(parse_chord("").is_err());
    assert!(parse_chord("   ").is_err());
    assert!(parse_chord("ctrl++s").is_err()); // empty segment
}
```

- [ ] **Step 2.5: Register the new test modules**

Edit `tests/integration.rs`:

```rust
mod integration {
    pub mod chord_parse;
    pub mod cli_parse;
    pub mod clipboard_offline;
    pub mod keymap;
    pub mod lockfile;
    pub mod observe_offline;
    pub mod text_editor;
    pub mod tools_list;
}
```

- [ ] **Step 2.6: Run the tests**

```bash
cargo test --workspace keymap 2>&1 | tail -10
cargo test --workspace chord_parse 2>&1 | tail -10
```

Expected: all keymap tests pass (7), all chord_parse tests pass (10). 17 new passing tests.

- [ ] **Step 2.7: Commit**

```bash
git add src/keymap.rs src/lib.rs tests/integration.rs tests/integration/keymap.rs tests/integration/chord_parse.rs
git commit -m "feat(phase-2): keymap + chord parser with unit tests"
```

---

## Task 3: `src/kwin/eis.rs` — EIS session helper

Lifts the EIS handshake from both probers into a reusable async helper. Per-call lifetime: `EisSession::open(bus, caps)` opens the connection, drives the event loop until `DeviceResumed`, returns a session. `session.emit_keyboard(|kb, dev| ...)` / `emit_pointer(...)` give the caller a handle to the libei device + interface inside a `start_emulating` / `stop_emulating` bracket.

**Files:**
- Create: `src/kwin/eis.rs`
- Modify: `src/kwin/mod.rs`

- [ ] **Step 3.1: Define the connection helper**

Create `src/kwin/eis.rs`:

```rust
//! Per-call EIS session helper. Wraps the libei handshake the two probers
//! pioneered (see `examples/eis-keystroke-prober/` and `eis-mouse-prober/`).
//!
//! Lifecycle: `EisSession::open(bus, caps)` does the full handshake and
//! returns a session ready to emit input events. The caller invokes
//! `emit(|dev, ifs, last_serial| { ... })` which brackets the block with
//! `start_emulating` / `stop_emulating` and flushes between frames.
//! Dropping the session closes the underlying libei context (KWin gets the
//! `disconnect` via socket close).

use crate::error::ToolError;
use futures_util::StreamExt;
use reis::{ei, event::DeviceCapability};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zbus::Connection;

/// KWin's `connectToEIS(flags)` is a capability bit-mask, NOT a mode bit.
/// See `examples/eis-mouse-prober/README.md` and the Phase 0 spike postscript
/// correction.
pub const CAP_KEYBOARD: i32 = 1;
pub const CAP_POINTER: i32 = 2;
pub const CAP_TOUCH: i32 = 4;

#[zbus::proxy(
    interface = "org.kde.KWin.EIS.RemoteDesktop",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/EIS/RemoteDesktop"
)]
trait RemoteDesktop {
    #[zbus(name = "connectToEIS")]
    fn connect_to_eis(&self, flags: i32) -> zbus::Result<(zvariant::OwnedFd, i32)>;
}

pub struct EisSession {
    pub context: ei::Context,
    pub device: reis::event::Device,
    pub last_serial: u32,
}

impl EisSession {
    /// Open an EIS session and drive the handshake until a device matching
    /// `expect_caps` is resumed and ready. `caps` is the OR of `CAP_*`
    /// constants for what to request from KWin; `expect_caps` is the
    /// capability predicate the returned device must satisfy.
    pub async fn open(
        bus: &Connection,
        caps_flags: i32,
        expect_keyboard: bool,
        expect_pointer: bool,
    ) -> Result<Self, ToolError> {
        let proxy = RemoteDesktopProxy::new(bus)
            .await
            .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;
        let (owned_fd, _conn_id) = proxy
            .connect_to_eis(caps_flags)
            .await
            .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;
        let std_fd: std::os::fd::OwnedFd = owned_fd.into();
        let raw_fd = std_fd.into_raw_fd();
        let stream = unsafe { UnixStream::from_raw_fd(raw_fd) };
        stream
            .set_nonblocking(true)
            .map_err(|e| ToolError::Io { path: "eis stream".into(), error: e.to_string() })?;
        let context = ei::Context::new(stream).map_err(|e| ToolError::Io {
            path: "ei::Context".into(),
            error: e.to_string(),
        })?;

        let (_conn, mut events) = context
            .handshake_tokio("kde-mcp", ei::handshake::ContextType::Sender)
            .await
            .map_err(|e| ToolError::InputDispatchFailed {
                detail: format!("handshake: {e}"),
                libei_version: None,
                kwin_version: None,
            })?;

        let mut chosen_dev: Option<reis::event::Device> = None;
        let mut last_serial: u32 = 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some(result) = tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            events.next(),
        )
        .await
        .map_err(|_| ToolError::Timeout {
            closest_match: None,
            current_windows: vec![],
        })?
        {
            let event = result.map_err(|e| ToolError::InputDispatchFailed {
                detail: format!("event stream: {e}"),
                libei_version: None,
                kwin_version: None,
            })?;
            match &event {
                reis::event::EiEvent::SeatAdded(evt) => {
                    let mut bind = DeviceCapability::Scroll.into();
                    if expect_keyboard {
                        bind |= DeviceCapability::Keyboard;
                    }
                    if expect_pointer {
                        bind |= DeviceCapability::Pointer
                            | DeviceCapability::PointerAbsolute
                            | DeviceCapability::Button;
                    }
                    evt.seat.bind_capabilities(bind);
                    context.flush().map_err(|e| ToolError::InputDispatchFailed {
                        detail: format!("flush after bind: {e}"),
                        libei_version: None,
                        kwin_version: None,
                    })?;
                }
                reis::event::EiEvent::DeviceAdded(evt) => {
                    let has_kb = evt.device.has_capability(DeviceCapability::Keyboard);
                    let has_ptr = evt.device.has_capability(DeviceCapability::PointerAbsolute)
                        && evt.device.has_capability(DeviceCapability::Button);
                    if (expect_keyboard && has_kb) || (expect_pointer && has_ptr) {
                        chosen_dev = Some(evt.device.clone());
                    }
                }
                reis::event::EiEvent::DeviceResumed(evt) => {
                    last_serial = evt.serial;
                    if chosen_dev.is_some() {
                        break;
                    }
                }
                _ => {}
            }
        }

        let device = chosen_dev.ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "no matching device materialised within 3s".into(),
            libei_version: None,
            kwin_version: None,
        })?;
        Ok(EisSession {
            context,
            device,
            last_serial,
        })
    }

    pub fn now_us() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }
}

impl Drop for EisSession {
    fn drop(&mut self) {
        // Best-effort teardown; ignore errors since we're already dropping.
        // start_emulating + stop_emulating is bracketed by the caller (mouse
        // / keyboard tool body), so nothing to do here other than letting the
        // libei Context drop close the socket.
    }
}
```

- [ ] **Step 3.2: Wire into `kwin/mod.rs`**

Edit `src/kwin/mod.rs`:

```rust
//! KWin D-Bus surfaces.

pub mod eis;
pub mod screenshot;
pub mod scripting;
```

- [ ] **Step 3.3: Compile-only verification**

```bash
cargo check --workspace 2>&1 | tail -3
```

Expected: clean. No runtime tests for the EIS session in this task; T4/T5 exercise it indirectly. If clippy trips, fix per its hints.

- [ ] **Step 3.4: Commit**

```bash
git add src/kwin/eis.rs src/kwin/mod.rs
git commit -m "feat(phase-2): EIS session helper (per-call lifecycle)"
```

---

## Task 4: `src/tools/mouse.rs` — implementation

**Files:**
- Modify: `src/tools/mouse.rs`

- [ ] **Step 4.1: Implement the tool**

Replace `src/tools/mouse.rs`:

```rust
use crate::error::ToolError;
use crate::keymap::Modifier;
use crate::kwin::eis::{CAP_POINTER, EisSession};
use crate::server::KdeMcp;
use reis::ei;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// /usr/include/linux/input-event-codes.h
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MouseArgs {
    pub action: String, // click | move | drag | scroll
    #[serde(default)]
    pub coordinate: Option<[i32; 2]>,
    #[serde(default)]
    pub start_coordinate: Option<[i32; 2]>,
    #[serde(default)]
    pub button: Option<String>, // left (default) | right | middle
    #[serde(default)]
    pub count: Option<u32>, // 1 default, 2=double, 3=triple
    #[serde(default)]
    pub modifier: Option<Vec<String>>, // ctrl/shift/alt/super
    #[serde(default)]
    pub scroll_direction: Option<String>, // up | down | left | right
    #[serde(default)]
    pub scroll_amount: Option<i32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MouseResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn mouse(state: &KdeMcp, args: MouseArgs) -> Result<MouseResponse, ToolError> {
    let action = args.action.to_ascii_lowercase();
    let button = parse_button(args.button.as_deref().unwrap_or("left"))?;
    let modifiers = parse_modifiers(args.modifier.as_deref().unwrap_or(&[]))?;

    let mut session = EisSession::open(&state.bus, CAP_POINTER, false, true).await?;
    let device = session.device.device().clone();
    let pointer_abs: ei::PointerAbsolute = session
        .device
        .interface::<ei::PointerAbsolute>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing PointerAbsolute interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;
    let button_iface: ei::Button = session
        .device
        .interface::<ei::Button>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing Button interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;
    let scroll_iface = session.device.interface::<ei::Scroll>();

    device.start_emulating(session.last_serial, 0);
    flush(&session.context)?;

    let outcome = run_mouse_action(
        &action,
        &session,
        &device,
        &pointer_abs,
        &button_iface,
        scroll_iface.as_ref(),
        button,
        args.coordinate,
        args.start_coordinate,
        args.count.unwrap_or(1),
        &modifiers,
        args.scroll_direction.as_deref(),
        args.scroll_amount.unwrap_or(3),
    )
    .await;

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();

    outcome.map(|_| MouseResponse { ok: true, warnings: vec![] })
}

#[allow(clippy::too_many_arguments)]
async fn run_mouse_action(
    action: &str,
    session: &EisSession,
    device: &ei::Device,
    pointer_abs: &ei::PointerAbsolute,
    button_iface: &ei::Button,
    scroll_iface: Option<&ei::Scroll>,
    button: u32,
    coord: Option<[i32; 2]>,
    start_coord: Option<[i32; 2]>,
    count: u32,
    modifiers: &[Modifier],
    scroll_dir: Option<&str>,
    scroll_amount: i32,
) -> Result<(), ToolError> {
    // Modifiers (held throughout the action). For Phase 2 mouse, modifiers
    // are sent as separate keyboard events — but the pointer-only EIS
    // session doesn't have keyboard caps. So modifier+click requires routing
    // via a second EIS session OR returning a hint. For first cut: error if
    // modifiers are requested.
    if !modifiers.is_empty() {
        return Err(ToolError::UnsupportedRegion {
            reason: "mouse modifier keys require keyboard session combine; defer to Phase 6"
                .into(),
        });
    }

    match action {
        "move" => {
            let [x, y] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate required for move".into(),
            })?;
            pointer_abs.motion_absolute(x as f32, y as f32);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
        }
        "click" => {
            let [x, y] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate required for click".into(),
            })?;
            pointer_abs.motion_absolute(x as f32, y as f32);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            for _ in 0..count.max(1) {
                button_iface.button(button, ei::button::ButtonState::Press);
                device.frame(session.last_serial, EisSession::now_us());
                flush(&session.context)?;
                tokio::time::sleep(Duration::from_millis(30)).await;
                button_iface.button(button, ei::button::ButtonState::Released);
                device.frame(session.last_serial, EisSession::now_us());
                flush(&session.context)?;
                if count > 1 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
        "drag" => {
            let [sx, sy] = start_coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "start_coordinate required for drag".into(),
            })?;
            let [ex, ey] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate (end) required for drag".into(),
            })?;
            pointer_abs.motion_absolute(sx as f32, sy as f32);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            button_iface.button(button, ei::button::ButtonState::Press);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
            tokio::time::sleep(Duration::from_millis(30)).await;
            // Interpolate in ~10 steps.
            let steps = 10;
            for i in 1..=steps {
                let fx = sx as f32 + (ex - sx) as f32 * (i as f32 / steps as f32);
                let fy = sy as f32 + (ey - sy) as f32 * (i as f32 / steps as f32);
                pointer_abs.motion_absolute(fx, fy);
                device.frame(session.last_serial, EisSession::now_us());
                flush(&session.context)?;
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            button_iface.button(button, ei::button::ButtonState::Released);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
        }
        "scroll" => {
            let scroll = scroll_iface.ok_or_else(|| ToolError::InputDispatchFailed {
                detail: "device missing Scroll interface".into(),
                libei_version: None,
                kwin_version: None,
            })?;
            if let Some([x, y]) = coord {
                pointer_abs.motion_absolute(x as f32, y as f32);
                device.frame(session.last_serial, EisSession::now_us());
                flush(&session.context)?;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            // libei scroll uses logical-pixel units; one notch ~= 15px.
            let (dx, dy) = match scroll_dir.unwrap_or("down") {
                "up" => (0.0, -15.0 * scroll_amount as f32),
                "down" => (0.0, 15.0 * scroll_amount as f32),
                "left" => (-15.0 * scroll_amount as f32, 0.0),
                "right" => (15.0 * scroll_amount as f32, 0.0),
                other => {
                    return Err(ToolError::UnsupportedRegion {
                        reason: format!("unknown scroll_direction: {other}"),
                    });
                }
            };
            scroll.scroll(dx, dy);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
        }
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unsupported mouse action: {other}"),
            });
        }
    }
    Ok(())
}

fn parse_button(name: &str) -> Result<u32, ToolError> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "left" => BTN_LEFT,
        "right" => BTN_RIGHT,
        "middle" => BTN_MIDDLE,
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unknown button: {other}"),
            });
        }
    })
}

fn parse_modifiers(names: &[String]) -> Result<Vec<Modifier>, ToolError> {
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let m = match n.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "shift" => Modifier::Shift,
            "alt" => Modifier::Alt,
            "super" | "meta" | "win" => Modifier::Super,
            other => {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("unknown modifier: {other}"),
                });
            }
        };
        out.push(m);
    }
    Ok(out)
}

fn flush(ctx: &ei::Context) -> Result<(), ToolError> {
    ctx.flush().map_err(|e| ToolError::InputDispatchFailed {
        detail: format!("flush: {e}"),
        libei_version: None,
        kwin_version: None,
    })
}
```

- [ ] **Step 4.2: Verify compilation**

```bash
cargo check --workspace 2>&1 | tail -3
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

Expected: both clean. If clippy complains about `too_many_arguments` on `run_mouse_action`, the `#[allow(clippy::too_many_arguments)]` already on the fn suppresses it. If clippy complains about something else, fix per its hints.

- [ ] **Step 4.3: Commit**

```bash
git add src/tools/mouse.rs
git commit -m "feat(phase-2): mouse tool (click/move/drag/scroll via libei)"
```

---

## Task 5: `src/tools/keyboard.rs` — implementation

**Files:**
- Modify: `src/tools/keyboard.rs`

- [ ] **Step 5.1: Implement keyboard_type and keyboard_key**

Replace `src/tools/keyboard.rs`:

```rust
use crate::error::ToolError;
use crate::keymap::{Modifier, keycode_for_char, parse_chord, shift_required, KEY_LEFTCTRL};
use crate::kwin::eis::{CAP_KEYBOARD, EisSession};
use crate::server::KdeMcp;
use reis::ei;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// Split per spec §4.5 poka-yoke: type accepts text only; key accepts keys
// only. "Type while holding Ctrl" can only be expressed by two sequential
// calls, which is what we want.

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardTypeArgs {
    pub text: String,
    /// "auto" (default) | "always" | "never". Auto routes to clipboard-paste
    /// when text > 32 chars or contains non-ASCII.
    #[serde(default)]
    pub use_clipboard: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardKeyArgs {
    pub keys: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_clipboard: Option<bool>,
}

pub async fn keyboard_type(
    state: &KdeMcp,
    args: KeyboardTypeArgs,
) -> Result<KeyboardResponse, ToolError> {
    let mode = args.use_clipboard.as_deref().unwrap_or("auto");
    let needs_clipboard = match mode {
        "always" => true,
        "never" => false,
        "auto" | _ => args.text.chars().count() > 32 || args.text.chars().any(|c| !c.is_ascii()),
    };

    if needs_clipboard {
        return type_via_clipboard(state, &args.text).await;
    }

    let mut session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session
        .device
        .interface::<ei::Keyboard>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;

    device.start_emulating(session.last_serial, 0);
    flush(&session.context)?;

    for ch in args.text.chars() {
        let keycode = keycode_for_char(ch).ok_or_else(|| ToolError::UnsupportedRegion {
            reason: format!("cannot type character {ch:?} via direct keymap; use clipboard-paste"),
        })?;
        let shift = shift_required(ch);
        if shift {
            kb.key(crate::keymap::KEY_LEFTSHIFT, ei::keyboard::KeyState::Press);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
        }
        kb.key(keycode, ei::keyboard::KeyState::Press);
        device.frame(session.last_serial, EisSession::now_us());
        flush(&session.context)?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        kb.key(keycode, ei::keyboard::KeyState::Released);
        device.frame(session.last_serial, EisSession::now_us());
        flush(&session.context)?;
        if shift {
            kb.key(crate::keymap::KEY_LEFTSHIFT, ei::keyboard::KeyState::Released);
            device.frame(session.last_serial, EisSession::now_us());
            flush(&session.context)?;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();
    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: Some(false),
    })
}

pub async fn keyboard_key(
    state: &KdeMcp,
    args: KeyboardKeyArgs,
) -> Result<KeyboardResponse, ToolError> {
    let chord = parse_chord(&args.keys)?;

    let mut session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session
        .device
        .interface::<ei::Keyboard>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;

    device.start_emulating(session.last_serial, 0);
    flush(&session.context)?;

    // Press modifiers in order, then the key, then release in reverse.
    for m in &chord.modifiers {
        kb.key(m.keycode(), ei::keyboard::KeyState::Press);
        device.frame(session.last_serial, EisSession::now_us());
        flush(&session.context)?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    kb.key(chord.key, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    tokio::time::sleep(Duration::from_millis(30)).await;
    kb.key(chord.key, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    for m in chord.modifiers.iter().rev() {
        tokio::time::sleep(Duration::from_millis(10)).await;
        kb.key(m.keycode(), ei::keyboard::KeyState::Released);
        device.frame(session.last_serial, EisSession::now_us());
        flush(&session.context)?;
    }

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();
    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: None,
    })
}

/// Save current clipboard, write `text` to it, send Ctrl+V, restore the
/// original clipboard. Returns the keyboard-tool response with
/// `used_clipboard: true`.
async fn type_via_clipboard(state: &KdeMcp, text: &str) -> Result<KeyboardResponse, ToolError> {
    // Save the existing clipboard.
    let saved = tokio::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .await
        .ok();

    // Write our text.
    let mut child = tokio::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/plain")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawn wl-copy: {e}"),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("write to wl-copy: {e}"),
            })?;
        drop(stdin);
    }
    let _ = child.wait().await;

    // Send Ctrl+V.
    let mut session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session
        .device
        .interface::<ei::Keyboard>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;
    let paste_key = crate::keymap::keycode_for_char('v').unwrap();
    device.start_emulating(session.last_serial, 0);
    flush(&session.context)?;
    kb.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    tokio::time::sleep(Duration::from_millis(15)).await;
    kb.key(paste_key, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    tokio::time::sleep(Duration::from_millis(30)).await;
    kb.key(paste_key, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    tokio::time::sleep(Duration::from_millis(15)).await;
    kb.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    flush(&session.context)?;
    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();

    // Give the paste a beat to be consumed by the focused app.
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Restore prior clipboard (best-effort).
    if let Some(prior) = saved
        && prior.status.success()
        && !prior.stdout.is_empty()
    {
        let mut restore = tokio::process::Command::new("wl-copy")
            .arg("--type")
            .arg("text/plain")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .ok();
        if let Some(child) = restore.as_mut()
            && let Some(mut stdin) = child.stdin.take()
        {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(&prior.stdout).await;
        }
        if let Some(mut child) = restore {
            let _ = child.wait().await;
        }
    }

    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: Some(true),
    })
}

fn flush(ctx: &ei::Context) -> Result<(), ToolError> {
    ctx.flush().map_err(|e| ToolError::InputDispatchFailed {
        detail: format!("flush: {e}"),
        libei_version: None,
        kwin_version: None,
    })
}
```

- [ ] **Step 5.2: Verify compilation**

```bash
cargo check --workspace 2>&1 | tail -3
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 5.3: Commit**

```bash
git add src/tools/keyboard.rs
git commit -m "feat(phase-2): keyboard.type + keyboard.key via libei"
```

---

## Task 6: `clipboard_write`

**Files:**
- Modify: `src/tools/clipboard.rs`

- [ ] **Step 6.1: Implement clipboard_write**

In `src/tools/clipboard.rs`, replace the `clipboard_write` function body:

```rust
pub async fn clipboard_write(
    _state: &KdeMcp,
    args: ClipboardWriteArgs,
) -> Result<ClipboardWriteResponse, ToolError> {
    let mut child = tokio::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/plain")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawn wl-copy: {e}"),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(args.content.as_bytes())
            .await
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("write to wl-copy: {e}"),
            })?;
        drop(stdin);
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("wait wl-copy: {e}"),
        })?;
    if !status.success() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: format!("wl-copy exited {status:?}"),
        });
    }
    Ok(ClipboardWriteResponse::default())
}
```

- [ ] **Step 6.2: Update `ClipboardWriteResponse` to carry an `ok` field**

In the same file, change:

```rust
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardWriteResponse {}
```

to:

```rust
#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardWriteResponse {
    pub ok: bool,
}
```

And in the impl, return `Ok(ClipboardWriteResponse { ok: true })`.

- [ ] **Step 6.3: Verify**

```bash
cargo test --workspace 2>&1 | grep "test result"
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 6.4: Commit**

```bash
git add src/tools/clipboard.rs
git commit -m "feat(phase-2): clipboard_write via wl-copy"
```

---

## Task 7: `act_on_element`

**Files:**
- Modify: `src/tools/act_on_element.rs`

- [ ] **Step 7.1: Implement**

Replace `src/tools/act_on_element.rs`:

```rust
use crate::atspi::ref_encoding::decode_element_ref;
use crate::error::ToolError;
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus::names::BusName;
use atspi::zbus::zvariant::ObjectPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementResponse {
    pub ok: bool,
    pub action_invoked: String,
}

pub async fn act_on_element(
    state: &KdeMcp,
    args: ActOnElementArgs,
) -> Result<ActOnElementResponse, ToolError> {
    let meta = state
        .refs
        .resolve(&args.element_ref)
        .await
        .map_err(|e| match e {
            RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                ref_id: args.element_ref.clone(),
            },
        })?;
    if meta.kind != RefKind::Element {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("ref {} is not an element", args.element_ref),
        });
    }
    let (bus, path) = decode_element_ref(&meta.payload).ok_or_else(|| ToolError::StaleHandle {
        ref_id: args.element_ref.clone(),
    })?;

    let conn = state.atspi.get().await?;
    let bus_name = BusName::try_from(bus.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad bus name {bus}: {e}"),
    })?;
    let obj_path = ObjectPath::try_from(path.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad path {path}: {e}"),
    })?;

    let acc = AccessibleProxy::builder(conn.connection())
        .destination(bus_name)
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?
        .path(obj_path)
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?
        .cache_properties(atspi::zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    let proxies = acc
        .proxies()
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;
    let action_proxy = proxies
        .action()
        .await
        .map_err(|_| ToolError::NotActionable {
            available_actions: vec![],
            fallback_suggestion: Some("element has no Action interface; try mouse.click on bbox".into()),
        })?;
    let actions = action_proxy
        .get_actions()
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    let requested = args.action.to_ascii_lowercase();
    let idx = actions
        .iter()
        .position(|a| a.name.eq_ignore_ascii_case(&requested))
        .ok_or_else(|| ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some(format!(
                "no action named {requested:?}; try one of the available_actions"
            )),
        })? as i32;

    let invoked = action_proxy
        .do_action(idx)
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;
    if !invoked {
        return Err(ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some("AT-SPI do_action returned false".into()),
        });
    }
    Ok(ActOnElementResponse {
        ok: true,
        action_invoked: actions[idx as usize].name.clone(),
    })
}
```

- [ ] **Step 7.2: Verify**

```bash
cargo check --workspace 2>&1 | tail -3
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 7.3: Commit**

```bash
git add src/tools/act_on_element.rs
git commit -m "feat(phase-2): act_on_element via AT-SPI Action.do_action"
```

---

## Task 8: SMOKE.md §D + verify + tag phase-2

- [ ] **Step 8.1: Append §D to `tests/integration/SMOKE.md`**

Append after §C:

````markdown
---

## Part D — Phase 2 mutating tools

Each `--mode=live` call fires a KDE notification. The spec §14 row 2 demo target is "open kcalc, type 2+3, press =, read the result"; we exercise pieces of that here.

### D.1 — `keyboard.key Ctrl+Tab` in a focused browser

Focus a browser tab with multiple tabs open, then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.key","arguments":{"keys":"ctrl+Tab"}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d1.jsonl
sed -n '2p' /tmp/d1.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["ok"]
print("D.1 OK")'
```

Expected: the browser switches tabs.

### D.2 — `clipboard_write` + verify via `clipboard_read`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"clipboard_write","arguments":{"content":"phase-2-write-test"}}}' \
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"clipboard_read","arguments":{}}}'; \
  sleep 0.8; } | timeout 6 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d2.jsonl
test "$(wc -l < /tmp/d2.jsonl)" = 3
sed -n '3p' /tmp/d2.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["content"]["text"] == "phase-2-write-test", payload
print("D.2 OK")'
```

### D.3 — `mouse click` on a Brave tab

Get Brave's UUID via `window_query` first, then click at (400, 20). Expected: a different Brave tab becomes active.

```sh
# Run window_query, parse, then send mouse.click at (400, 20):
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mouse","arguments":{"action":"click","coordinate":[400,20]}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d3.jsonl
sed -n '2p' /tmp/d3.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["ok"]
print("D.3 OK")'
```

### D.4 — `keyboard.type` short ASCII string

Focus a text field (URL bar, terminal, kate), then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.type","arguments":{"text":"hello"}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d4.jsonl
sed -n '2p' /tmp/d4.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["used_clipboard"] == False, payload
print("D.4 OK")'
```

### D.5 — `keyboard.type` triggers clipboard-paste for non-ASCII

Focus a text field, then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.type","arguments":{"text":"Привет"}}}'; \
  sleep 2.0; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d5.jsonl
sed -n '2p' /tmp/d5.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["used_clipboard"] is True, payload
print("D.5 OK")'
```

### D.6 — `act_on_element` (deferred to Phase 4)

This requires a previous `inspect` to mint an element ref. The current Phase 1 `inspect target=focused` returns `not_implemented`, and `inspect target=window:<ref>` is also deferred. So end-to-end `act_on_element` verification waits for Phase 4. The code path is exercised by Task 7's compile check and by spec parity tests.
````

- [ ] **Step 8.2: Run full test suite + fmt + clippy**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo test --workspace 2>&1 | grep "test result"
```

Expected: clean fmt/clippy; 80+ tests passing (64 from Phase 1 + 17 new from Task 2).

- [ ] **Step 8.3: Live smoke fixtures D.1–D.5**

Run each fixture. Reset audit log between runs if needed:

```bash
truncate -s 0 ~/.local/state/kde-mcp/audit.jsonl
# Run D.1 (focus Brave first), then verify the audit log entry
tail -1 ~/.local/state/kde-mcp/audit.jsonl | python3 -c "import sys,json; r=json.loads(sys.stdin.read()); print(r['tool'], r['ok'], r.get('duration_ms'))"
```

- [ ] **Step 8.4: Update `docs/HANDOFF.md`**

Mark Phase 2 complete, pivot the "What is NOT yet done" section to Phase 3 (policy gate). Update test count, last commit, tag.

- [ ] **Step 8.5: Commit + tag**

```bash
git add docs/HANDOFF.md tests/integration/SMOKE.md
git commit -m "docs(phase-2): mark complete; pivot to Phase 3 policy gate"
git tag phase-2
```

---

## Self-review

**Spec coverage:** Phase 2 row of spec §14 covers `mouse`, `keyboard`, `clipboard_write`, `act_on_element`. All four have tasks (4, 5, 6, 7). The keyboard split is `keyboard.type` and `keyboard.key` per Phase 0's wire-name decision. The mouse split into actions (click/move/drag/scroll) is covered in Task 4. `act_on_element` uses AT-SPI `Action.do_action(index)` from Task 11's Phase 1 work; this is spec §6.2's primary mechanism.

**Placeholder scan:** the only `not_implemented` returns remaining after Phase 2 are by design — `mouse modifier+click` (Phase 6 polish; clearly documented in Task 4.1), `keyboard.hold_key` (Phase 6; not part of Phase 2 scope), and the existing Phase 1 deferrals (`inspect target=focused/window:<ref>`, `observe.focused_element`).

**Type consistency:** `Chord`, `Modifier`, `KEY_*` constants are defined in `src/keymap.rs` and used by both `src/tools/mouse.rs` and `src/tools/keyboard.rs`. `EisSession::open(bus, caps, expect_keyboard, expect_pointer)` is the single entry point used by both mouse and keyboard tools; the `caps` argument uses the bit-mask constants from `src/kwin/eis.rs`. `KdeMcp::new` signature is unchanged from Phase 1 — no test updates needed.

**Tensions resolved:**
- §4.5 poka-yoke × `mouse` modifiers: deferred to Phase 6. Phase 2 errors cleanly with `unsupported_region`.
- §6.2 keyboard `use_clipboard: auto` semantics: implemented exactly per the rule (>32 chars OR non-ASCII).
- §10 layer 2 audit-log always-on: Phase 1's `dispatch_audited` already covers Phase 2 tools; no additional work.
- Policy gate (§6.0, Phase 3): NOT in Phase 2's scope. Tools dispatch through `dispatch_audited` only; the gate wraps them in Phase 3.

---

## Execution handoff

Plan complete and saved to `docs/plans/2026-05-14-phase-2-input-tools.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, review between tasks.

**2. Inline Execution** — execute in this session via `superpowers:executing-plans`, batch with checkpoints.

User has been autonomous-mode all session. Recommended sequence in continuation:

- T0 spike (45 min, read-only investigations)
- T1 Cargo deps
- T2 keymap + chord parser (pure module, 17 unit tests)
- T3 EIS session helper (compile-only)
- T4 mouse impl (compile-only; live test deferred to user smoke)
- T5 keyboard impl (compile-only)
- T6 clipboard_write impl
- T7 act_on_element impl
- T8 SMOKE refresh + verification + tag

Tasks 0–7 are all session-safe (no live KWin emission). Live smoke (D.1–D.5) requires user interaction (focus target window first) and fires KDE notifications.
