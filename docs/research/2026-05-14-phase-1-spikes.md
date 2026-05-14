# Phase 1 Spike Findings

Captured 2026-05-14, before Phase 1 implementation begins. Read-only investigations only.

## Summary of plan adjustments

| Source | Adjustment |
|---|---|
| Spike 0.1 | `AccessibilityConnection::new()` constructor confirmed; no direct "focused" property — Phase 1 ships `observe.focused_element = null + warning` for the first cut. Tree walker (Task 11) uses `Accessible::{name, get_role, child_count, get_child_at_index, parent}` + the separate `Component` proxy for bbox + `Action` proxy for actions. |
| Spike 0.2 | **JS rename:** `workspace.activeOutput` → `workspace.activeScreen` in `JS_OBSERVE` (Task 13.1). Drop the `ws.windows` fallback (`windowList()` is always present on Plasma 6 via `QtScriptWorkspaceWrapper`). |
| Spike 0.3 | ScreenShot2 signatures confirmed exactly as the Task 7 plan drafted them. `Version` property reads `5`. |
| Spike 0.4 | **`wl-clipboard` not installed on dev machine.** Offline test passes via `external_command_failed` branch; live spot-check (Task 4.5) is gated on user installing the package (`pacman -S wl-clipboard` on Arch). |

## 0.1 `atspi 0.30` API surface for focused-element retrieval

**Sources:**
- `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi-connection/src/lib.rs`
- `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi-proxies/src/accessible.rs`
- `https://docs.rs/atspi/0.30.0/atspi/`

### Connection type

```rust
pub struct AccessibilityConnection { /* internal */ }

impl AccessibilityConnection {
    pub async fn new() -> AtspiResult<Self>;
    pub async fn from_address(bus_addr: Address) -> AtspiResult<Self>;

    // Requires the "wrappers" feature (on by default for the `atspi`
    // re-export crate).
    pub fn event_stream(&self) -> impl Stream<Item = Result<Event, AtspiError>>;
}
```

Re-exported as `atspi::AccessibilityConnection`. With the `tokio` feature
enabled (`features = ["tokio"]`) the underlying zbus transport uses tokio.

### Focus tracking

**No direct "give me the focused accessible" property exists.** The crate
exposes a `FocusEvents` enum and `EventProperties` trait; focus is observed
via signals on `org.a11y.atspi`. The decision the spec already implied:
Phase 1's `observe` and `inspect "focused"` paths must subscribe to the
event stream and remember the last focused element in an
`AtomicCell<Option<ObjectRef>>` owned by `KdeMcp`.

For Phase 1's first cut we can take a simpler path: skip
`focused_element` in `observe` when the focus tracker hasn't yet seen a
focus event (return `null` with a warning), and implement `inspect
target="focused"` as "root → walk-by-state Active|Focused". This avoids
running a background signal task in Phase 1 and lets the agent get a
useful result on the first invocation.

### `Accessible` proxy methods

The proxy generated from `atspi-proxies/src/accessible.rs`:

| Method | Signature | Phase 1 use |
|---|---|---|
| `name` | `fn name(&self) -> zbus::Result<String>` | `observe.focused_element.name`, `inspect.elements[].name` |
| `description` | `fn description(&self) -> zbus::Result<String>` | Not used in Phase 1 |
| `get_role` | `fn get_role(&self) -> zbus::Result<Role>` | `inspect.elements[].role` |
| `child_count` | `fn child_count(&self) -> zbus::Result<i32>` | tree walk |
| `get_child_at_index` | `fn get_child_at_index(&self, index: i32) -> zbus::Result<ObjectRefOwned>` | tree walk |
| `parent` | `fn parent(&self) -> zbus::Result<ObjectRefOwned>` | parent_id encoding |
| `get_index_in_parent` | `fn get_index_in_parent(&self) -> zbus::Result<i32>` | tree walk back-pointer |
| `accessible_id` | `fn accessible_id(&self) -> zbus::Result<String>` | optional in `id` field |

`Component` proxy (for `bbox`) and `Action` proxy (for `actions`) are
separate traits in `atspi-proxies` — Phase 1's tree walker constructs all
three proxies for each accessible (cheap; same connection).

**Decision for Task 11 (tree walker):** the walker is implemented exactly
against the methods above. The `Role` enum has a `to_string()` per the
`atspi-common` re-exports. `States` come from a `StateSet` (also in
`atspi-common`) with a `.iter()` over set bits.

**Decision for Task 13 (`observe`):** the focused-element field is `null`
with a warning in Phase 1's first cut. Background focus-tracking lands in
Phase 4 polish if profiling shows a real cost from on-demand tree walks.

---


## 0.2 KWin Plasma 6 Scripting global names

**Sources:**
- `https://raw.githubusercontent.com/KDE/kwin/master/src/scripting/workspace_wrapper.h`
- `https://raw.githubusercontent.com/KDE/kwin/master/src/window.h`

### Confirmed `workspace.*` surface

| JS expression | Q_PROPERTY / Q_INVOKABLE | Notes |
|---|---|---|
| `workspace.activeWindow` | `Q_PROPERTY(KWin::Window* activeWindow READ activeWindow ...)` | The single property to use. Plasma-5 `activeClient` is **gone** on master. |
| `workspace.windowList()` | `Q_INVOKABLE QList<KWin::Window*> windowList() const` | Method, **not** a property. JS calls it as a function. The DeclarativeScriptWorkspaceWrapper subclass exposes a `windows` property; the QtScriptWorkspaceWrapper subclass (used by `loadScript`) exposes `windowList()`. **Use `windowList()`.** |
| `workspace.cursorPos` | `Q_PROPERTY(QPoint cursorPos ...)` | Returns a `QPoint`; JS sees `.x`, `.y`. |
| `workspace.screens` | `Q_PROPERTY(QList<KWin::LogicalOutput*> screens ...)` | List of outputs. |
| `workspace.activeScreen` | `Q_PROPERTY(KWin::LogicalOutput* activeScreen ...)` | **Note:** named `activeScreen`, not `activeOutput`. The plan's draft JS used `activeOutput` — correction applied below. |

### Confirmed `Window.*` per-window properties

All present on `KWin::Window` (file: `src/window.h`):

- `internalId` (QUuid, CONSTANT) — `w.internalId.toString()` gives the UUID string used as our `RefKind::Window` payload.
- `caption` (QString)
- `resourceClass` (QString) — primary "app class" we filter on for terminal masking.
- `resourceName` (QString)
- `pid` (int, CONSTANT)
- `frameGeometry` (KWin::RectF) — `.x`, `.y`, `.width`, `.height` on JS side.
- `output` (LogicalOutput*)
- `desktops` (QList<VirtualDesktop*>) — `w.desktops[0].x11DesktopNumber` works.
- `minimized` (bool, READ isMinimized)
- `fullScreen` (bool, READ isFullScreen)
- `maximizeMode` (KWin::MaximizeMode) — non-zero ⇒ maximised.

### Plan adjustments

1. **`workspace.activeOutput` → `workspace.activeScreen`** in `JS_OBSERVE` (Task 13.1). The `(ws.activeOutput === s)` line becomes `(ws.activeScreen === s)`.
2. **Drop the `ws.windows` fallback** in `JS_LIST_ALL` and `JS_OBSERVE`. On Plasma 6 it's always `ws.windowList()`. Keeping the fallback is harmless but adds a dead branch.

The Phase 1 plan's existing JS bodies already use `workspace.activeWindow`, `windowList()` (with `ws.windows` fallback), `cursorPos`, and the right per-window property names. Only the `activeOutput → activeScreen` rename is load-bearing.

---


## 0.3 `org.kde.KWin.ScreenShot2` D-Bus interface (live introspect)

**Source:** `busctl --user introspect org.kde.KWin /org/kde/KWin/ScreenShot2 org.kde.KWin.ScreenShot2` on the dev machine, 2026-05-14.

```
NAME                 TYPE     SIGNATURE  RESULT/VALUE FLAGS
.CaptureActiveScreen method   a{sv}h     a{sv}        -
.CaptureActiveWindow method   a{sv}h     a{sv}        -
.CaptureArea         method   iiuua{sv}h a{sv}        -
.CaptureInteractive  method   ua{sv}h    a{sv}        -
.CaptureScreen       method   sa{sv}h    a{sv}        -
.CaptureWindow       method   sa{sv}h    a{sv}        -
.CaptureWorkspace    method   a{sv}h     a{sv}        -
.Version             property u          5            emits-change
```

**Reading the signatures:**

- `a{sv}` — `HashMap<String, Variant>` options dict.
- `h` — UNIX_FD passed in as the *last* input arg (the write end of a pipe the caller created).
- Returns `a{sv}` — a results dict (typical keys: `"width"`, `"height"`, `"scale"`, `"windowId"`).

| Method | Input | Notes |
|---|---|---|
| `CaptureWorkspace` | `(options, fd)` | Full multi-screen workspace. |
| `CaptureActiveScreen` | `(options, fd)` | Single active output. |
| `CaptureActiveWindow` | `(options, fd)` | Active focused window. |
| `CaptureWindow` | `(handle: s, options, fd)` | `handle` is the KWin `internalId` UUID as a string. |
| `CaptureScreen` | `(name: s, options, fd)` | `name` is the LogicalOutput's name (e.g. `"DP-1"`). |
| `CaptureArea` | `(x: i32, y: i32, width: u32, height: u32, options, fd)` | Rectangular area in logical pixels. |
| `CaptureInteractive` | `(kind: u32, options, fd)` | User picks; not used by Phase 1. |

**Plan adjustments**

1. The plan's draft for `src/kwin/screenshot.rs` (Task 7) declared the proxy methods in the order `(options, pipe)` — **correct** for `CaptureWorkspace`, `CaptureActiveWindow`, `CaptureActiveScreen`, and as `(handle, options, pipe)` for `CaptureWindow`. The `CaptureArea` plan signature matches as well. **No changes required.**
2. **`Version` property** is at `5` on this machine. Not load-bearing for Phase 1, but worth recording — if a future KWin update bumps this, the structure of the results dict may change.

Common options-dict keys (from KWin source convention; can't introspect dict contents):
- `"include-cursor"` (bool) — defaults true.
- `"include-decoration"` (bool) — defaults true.
- `"native-resolution"` (bool) — defaults false; when true returns the raw output pixels rather than logical pixels.

Phase 1's `screenshot` always passes `"include-cursor"` per the tool's `include_cursor` arg and explicitly sets `"native-resolution": false` to keep the API's `[x, y, w, h]` coordinates in the same logical-pixel space the rest of the tools use.

---


## 0.4 `wl-paste` MIME negotiation and exit codes

**Source:** attempted live run on the dev machine, 2026-05-14.

```
$ which wl-paste
(not found)
$ wl-paste --version
zsh: command not found: wl-paste
$ wl-paste --type nonexistent/mime
zsh: command not found: wl-paste
exit=127
```

**Finding:** the `wl-clipboard` package is **not installed** on this development
machine. Spec §12 lists it as a system dependency (Arch: `sudo pacman -S
wl-clipboard`), so installation is the user's responsibility before live
verification of `clipboard_read`.

**Plan adjustments**

1. Task 4 (`clipboard_read`) is unblocked. The offline test
   (`tests/integration/clipboard_offline.rs`) accepts both
   `external_command_failed` and `target_not_found` as valid error codes —
   missing wl-paste produces the first.
2. Task 4 step 4.5 (live spot-check) is **gated on the user installing
   wl-clipboard.** It cannot run yet. Marker for the user when they
   return: `pacman -S wl-clipboard` (Arch) or equivalent.
3. The `wl-paste --list-types` line-delimited-MIME assumption stays as
   drafted; verified against wl-clipboard's man page (rather than live)
   and unchanged from the spec.

**Reference behaviour** (from wl-clipboard upstream): `wl-paste --list-types`
emits one MIME per line on stdout, exit 0 if a selection exists, exit 1 if
the clipboard is empty (stderr: "No selection"). `wl-paste --type X` exits
1 with stderr if `X` is not in the offered MIME set.

---


