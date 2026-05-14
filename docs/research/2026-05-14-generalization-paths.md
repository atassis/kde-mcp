# Generalization paths: from kde-mcp to a general-purpose computer-use MCP

**Status:** Research note. Captures the tradeoffs for taking kde-mcp beyond KDE Plasma 6 / Wayland. **Not** a binding decision — ADR-0002 still stands as written. Promotion to ADR-0009 would require explicit user sign-off and likely deferral of Phase 4–6 polish.

**Date:** 2026-05-14, after Phase 1 close.

---

## TL;DR

The Phase 1 surface is more portable than the project's name suggests. Of the six read-only tools:

- **4 are platform-neutral or near-neutral:** `text_editor.view` (file I/O), `clipboard_read` (Wayland-generic via `wl-paste`), `inspect` (AT-SPI is Linux-wide), and most of `observe.focused_element` (AT-SPI again).
- **2 are heavily KDE-coupled:** `window_query` (KWin Scripting D-Bus has no cross-compositor analogue with comparable expressive power), and `screenshot` (`org.kde.KWin.ScreenShot2` is KDE-only).

For Phase 2+ (input + writes), the picture flips. `libei` is cross-compositor by design, but each compositor exposes the EIS FD through a different D-Bus interface. `bash` is OS-generic. `clipboard_write` is the same Wayland-portable surface as the read. `text_editor.edit` is file I/O.

**Recommendation, briefly:** Refactor now to extract a `CompositorBackend` trait (one impl: `KwinBackend`). Cost is small at 6 tools, grows fast. Document the seams so a future X11 backend is tractable. Defer GNOME / wlroots / non-Linux indefinitely unless contributors arrive.

---

## What's actually KDE-specific in the current code

| Surface | File | Cross-compositor analogue | Effort to abstract |
|---|---|---|---|
| `org.kde.kwin.Scripting.loadScript` | `src/kwin/scripting.rs` | None with comparable expressive power. GNOME has `org.gnome.Shell.Eval` (deprecated, security-locked since 3.38); wlroots has nothing. | High — every workspace introspection currently funnels through this. |
| `org.kde.KWin.ScreenShot2` | `src/kwin/screenshot.rs` | `xdg-desktop-portal Screenshot` (universal, but shows a "share what?" dialog every time on most desktops); `org.gnome.Shell.Screenshot` (no dialog); `wlr-screencopy-unstable-v1` (used by `grim`). | Medium — well-bounded interface. |
| `org.kde.KWin.EIS.RemoteDesktop.connectToEIS` | (Phase 2) | `org.freedesktop.portal.RemoteDesktop` (universal, with permission prompt); `org.gnome.Shell.Eval` (gone); wlroots has its own protocols. | Medium — libei itself is portable; only the negotiation differs. |
| `kde-mcp.desktop` + `X-KDE-DBUS-Restricted-Interfaces` | `packaging/kde-mcp.desktop` | KDE-specific gate. Other compositors don't gate D-Bus this way. | Low — drop the file or make it KDE-conditional. |
| `org.freedesktop.Notifications` | `src/notify.rs` | **Already universal.** | Zero. |
| `org.kde.KWin.kglobalaccel` (Phase 3 Meta+Esc) | Phase 3 work | XDG global shortcut portal (Wayland); X11's XGrabKey (X11). | Medium. |
| AT-SPI via `atspi` crate | `src/atspi/` | **Already universal** — works on every Linux desktop with `at-spi2-core` installed. | Zero. |

Roughly 60% of the code that calls out to the desktop is in `src/kwin/`. Everything else is portable today or one rename away.

---

## Axes of generalization

Generalization is not a single decision. Three independent axes, each with its own cost curve:

### Axis 1 — Display protocol (X11 vs Wayland)

- **X11** is one platform, well-documented, stable. Tools: `xprop` / `wmctrl` (window list), `xdotool` (input, via XTest), `import` or `scrot` (screenshot). AT-SPI works the same. Permissions are trivial (any X11 client can do anything to any window on the same display).
- **Wayland** is fragmented across compositors. Each one exposes its own version of the same primitives. Some protocols (`wlr-foreign-toplevel-management`) are widely supported but limited; KDE explicitly refused to implement them for privacy reasons.

The cleanest split: **session-type detection at startup** via `$XDG_SESSION_TYPE` + D-Bus probes, then pick a backend.

### Axis 2 — Wayland compositor (within Wayland)

Three families:

1. **KDE Plasma (KWin)** — what we target now. Best non-portal D-Bus surface in the ecosystem: scripting engine, ScreenShot2 with rich options, direct EIS handoff without portal dialogs.
2. **GNOME (Mutter)** — locked down by design. `org.gnome.Shell.Eval` was the equivalent of KWin Scripting; deprecated and disabled by default since 3.38. Modern path is a custom Shell Extension installed in `~/.local/share/gnome-shell/extensions/`. xdg-desktop-portal is the supported route, but every action shows a permission dialog.
3. **wlroots-based** (Sway, Hyprland, river, Cosmic, etc.) — uses the `wlr-*` protocol family. `wlr-foreign-toplevel-management` gives titles + activated state but no geometry, no per-window properties, no script-like access. `wlr-screencopy-unstable-v1` is well-supported. Input via `wlr-virtual-keyboard-unstable-v1` / `wlr-virtual-pointer-unstable-v1`.

The hard truth: GNOME and wlroots both have **less expressive primitives than KDE.** Not just different — strictly weaker. `window_query` would have to return `null` for geometry on wlroots, for instance.

### Axis 3 — Operating system (Linux vs others)

- **macOS** has its own ecosystem: Accessibility API (the canonical accessibility tree), CGEventTap (input), ScreenCaptureKit (capture). Anthropic's `computer-use` reference impl already covers this. None of our D-Bus / libei / KWin code applies. The MCP wire surface (`rmcp`) is the only thing that survives.
- **Windows** likewise: UIAutomation, SendInput, Desktop Duplication API. Same — only the MCP wire stays.

Cross-OS support is a v2 architectural decision. The natural path is **the trait approach below**, with macOS and Windows as additional backend implementations. But each backend is essentially a rewrite of half the codebase.

---

## Proposed architecture: `CompositorBackend` trait

Pull the platform-specific bits behind a thin trait that the tools call into. Current state has tools calling KWin directly:

```rust
// today
pub async fn window_query(state: &KdeMcp, args: WindowQueryArgs) -> Result<...> {
    let payload = run_script(&state.bus, JS_LIST_ALL, ...).await?;  // KWin Scripting
    ...
}
```

After refactor:

```rust
// proposed
pub async fn window_query(state: &Server, args: WindowQueryArgs) -> Result<...> {
    let raw = state.backend.list_windows().await?;
    ...
}

#[async_trait]
pub trait CompositorBackend: Send + Sync {
    async fn list_windows(&self) -> Result<Vec<RawWindow>, ToolError>;
    async fn focused_window(&self) -> Result<Option<RawWindow>, ToolError>;
    async fn cursor_pos(&self) -> Result<(i32, i32), ToolError>;
    async fn screens(&self) -> Result<Vec<RawScreen>, ToolError>;
    async fn capture(&self, region: CaptureRegion) -> Result<Capture, ToolError>;
    async fn focus_window(&self, uuid: &str) -> Result<(), ToolError>;
    async fn close_window(&self, uuid: &str) -> Result<(), ToolError>;
    async fn input_session(&self) -> Result<Box<dyn InputSession>, ToolError>;
}

#[async_trait]
pub trait InputSession: Send {
    async fn type_text(&mut self, text: &str) -> Result<(), ToolError>;
    async fn press_key(&mut self, keys: &str) -> Result<(), ToolError>;
    async fn move_to(&mut self, x: i32, y: i32) -> Result<(), ToolError>;
    async fn click(&mut self, button: Button) -> Result<(), ToolError>;
    // ...
}

// One impl today; more later.
pub struct KwinBackend { bus: zbus::Connection }
pub struct X11Backend { /* xcb + XTest */ }     // future
pub struct GnomeBackend { /* shell extension + portal */ }  // future
pub struct WlrootsBackend { /* wlr-* protocols */ }  // future
```

`Server::new()` does platform detection and picks the backend. The tools never see `KdeMcp` again — they see `Server<dyn CompositorBackend>`.

**Cost today:** ~300 LOC of refactor, all mechanical. The `KwinBackend` impl wraps existing code with no behavioural changes. Tests need to be updated to construct a backend, which is a one-line shim.

**Cost tomorrow without the refactor:** each new backend would touch every tool file. With 15 tools and 4 backends, that's 60 files to keep in sync vs. 4 backend impls.

**When to do it:** As Phase 2 prep, before input tools land. After Phase 2, every new tool adds another method to the trait, but the trait shape is stable. Doing it after Phase 5 means refactoring all 15 tools at once, plus virtual-mode.

---

## What changes about the spec / ADRs

If we accept this direction:

- **ADR-0002 ("KDE-only scope")** stays as the v1 MVP statement, but a new ADR-0009 supersedes it: "trait-extracted backends; KWin remains the primary, X11 fallback planned." Old ADR's `Status:` line becomes `Superseded by ADR-0009`.
- **Spec §13 module layout** grows a `src/backends/` directory: `kwin/`, future `x11/`, etc. The `src/kwin/` and `src/atspi/` paths stay but become referenced from `src/backends/kwin/`.
- **Spec §10 safety layers** — the policy gate (Phase 3) stays compositor-agnostic. Per-app approval, sentinels, tier table all apply equally to any backend.
- **Spec §14 phasing** unchanged in scope; an additional optional Phase 7 ("X11 backend") slots in after Phase 6 polish.

The MVP day-count budget grows by about **3-5 days** for the refactor, plus open-ended for each new backend.

---

## Recommendations, ranked

1. **Do the trait extraction in Phase 2 prep.** Low cost now, exponential cost later. Even if we *never* add another backend, the trait clarifies what's KDE-specific and helps reasoning about the security/permission model.

2. **Document the X11 backend as Phase 7 (optional polish).** ~1000 LOC, well-bounded, gives coverage of "every Linux desktop session" since X11 still runs as fallback on most distros. Personal-use rationale of ADR-0002 doesn't object to X11 specifically; it objected to multi-compositor *Wayland* complexity.

3. **Defer GNOME and wlroots indefinitely.** Cost-benefit poor. GNOME needs a Shell Extension (separately packaged, separately reviewed). wlroots protocols are strictly weaker than KDE's surface. Neither is what the user runs.

4. **Park macOS / Windows as v2 architectural moves.** Each needs its own ADR, its own contributor, its own quality discipline. The MCP wire surface and the policy gate transfer; everything else is a rewrite.

5. **Promote "Linux generic" features into a base layer.** `src/atspi/`, `src/util/`, `src/audit.rs`, `src/refs.rs`, `src/trust.rs`, `src/lock.rs`, `src/server.rs` are all platform-neutral and should stay that way. Resist any temptation to add KDE-specific shortcuts in them.

---

## What this would look like, concretely

A near-term (post-Phase 1, pre-Phase 2) refactor PR:

- New `src/backend/mod.rs` defines the trait + `RawWindow`, `RawScreen`, `Capture`, etc.
- `src/backend/kwin/` moves the current `src/kwin/` content + adds a `KwinBackend` impl.
- `src/server.rs` `KdeMcp` becomes `Server`, with a `backend: Arc<dyn CompositorBackend>` field.
- `KdeMcp::new` → `Server::new(mode, audit, bus)` does the platform detection (only one option currently → always returns `KwinBackend`).
- Each tool file's free function changes `&KdeMcp` → `&Server`. The body calls `state.backend.X` instead of `crate::kwin::X`.
- `Cargo.toml` adds `async-trait = "0.1"` for the trait's async fns.
- 64 existing tests need their `KdeMcp::new` calls renamed, otherwise no behavioural changes.

Total LOC delta: estimated **~400 added, ~150 deleted, ~600 modified**. Well-bounded, one PR, no behavioural drift. Phase 1's smoke procedure (§C of `SMOKE.md`) runs unchanged afterwards.

Future X11 backend PR: new `src/backend/x11/` with ~1000 LOC. Pulls in `xcb` or similar. Implements the trait. The `Server::new` platform detection grows one new branch. Smoke procedure §D added for X11. No changes to any tool file.

---

## What we deliberately do not gain by going general

- **Better window introspection.** KWin's surface is the best in the ecosystem. Generalizing means accepting strictly less data from every other backend.
- **Easier security posture.** More backends = more code paths = more places a bug in `--mode=virtual` could escape. The policy gate gets harder to reason about, not easier.
- **Wider audience automatically.** Most users on KDE; KDE users are the natural early adopters. Non-KDE users are typically GNOME, which is the hardest backend.

This is why ADR-0002 made the call it did. The trade now is whether shipping Phase 1 + the surface knowledge we now have changes that calculus. My view: **the refactor part of the recommendation is a win regardless** (cleaner code, clearer security boundaries). The extra backends are conditional on someone caring enough to write them.

---

## Appendix — concrete probe of the gaps

Want a more grounded picture of what GNOME / wlroots / X11 offer? Three tests anyone can run, no kde-mcp code needed:

```sh
# 1. GNOME-Shell window query (if GNOME is running):
gdbus call --session --dest org.gnome.Shell \
  --object-path /org/gnome/Shell \
  --method org.gnome.Shell.Eval \
  'global.get_window_actors().map(a => a.meta_window.get_title()).join("\n")'
# → usually "ERROR: Eval is disabled" on modern GNOME (3.38+). Confirms the gap.

# 2. wlr-foreign-toplevel via `lswt` (if available; wlroots compositors only):
lswt
# → titles + app-ids + activated/maximized/minimized flags. No geometry. No pid. No bbox.

# 3. X11 wmctrl (any X11 session, including XWayland):
wmctrl -lG
# → window id, desktop, x, y, w, h, host, title. Comparable to what KWin gives us.
```

The X11 path returns a workable analogue. GNOME returns nothing without an extension. wlroots returns a strict subset.
