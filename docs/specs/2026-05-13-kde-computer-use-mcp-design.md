# kde-mcp — Design Specification

**Status:** Draft
**Date:** 2026-05-13
**Author:** atassis
**Audience:** developers implementing the server; future contributors deciding whether to extend it.

---

## 1. Summary

`kde-mcp` is an MCP (Model Context Protocol) server, written in Rust, that gives any MCP-compatible AI agent the ability to observe and control a KDE Plasma 6 / Wayland desktop. It is the Linux-native analogue of [Anthropic's Computer Use tool](https://docs.claude.com/en/docs/build-with-claude/computer-use), with three substantive improvements:

1. **Accessibility-tree first.** Most actions are deterministic AT-SPI invocations, not pixel-based clicks. Screenshots are an explicit fallback, not the primary surface.
2. **Response-shaped behaviour.** Tool responses carry structured deltas (window/focus/element diffs, warnings, recovery state, trust markers) that drive the agent's next decision — minimising reliance on prompt-engineering instructions far away in context.
3. **Two-process mode model.** `--mode=live` controls the user's real desktop; `--mode=virtual` spawns a private KWin session for isolated automation. No runtime swap, no single-process dual-context fragility.

The server speaks MCP over stdio and exposes 15 tools across observation, action, window/session management, clipboard, shell, and filesystem-editing surfaces.

---

## 2. Motivation

Anthropic's Computer Use is X11-only (Xvfb in a Docker container, driven by `xdotool` and `scrot`). It does not run against modern Linux Wayland desktops. The only existing Linux-Wayland alternative, [isac322/kwin-mcp](https://github.com/isac322/kwin-mcp), is Python and exposes 30 thin-wrapper tools focused on KWin internals rather than agent-friendly primitives.

The user (the author) runs KDE Plasma 6 on Wayland (kwin_wayland) daily. The need: an MCP server they can use from Claude Code, Claude Desktop, Cline, Cursor, or Zed to drive their real desktop reliably — and a sandboxed variant for unattended automation. The market is open territory; no first-party Anthropic MCP wrapper of computer-use exists, and none of the surveyed prior art combines KDE-native primitives with the agent-first response shape pioneered by Playwright-MCP.

This document specifies the design of that tool.

---

## 3. Non-goals

Explicit scope limits, kept here so future contributors and the author's future selves do not erode them:

- **Multi-compositor.** GNOME (Mutter), wlroots-based compositors (sway, Hyprland, Wayfire), and X11 are out of scope. Cross-compositor abstraction has a high cost in surface area and a low payoff for a personal tool. May be revisited in v2.
- **Multi-platform.** Linux only. No macOS, no Windows.
- **Browser automation.** Use [microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp).
- **Sandboxed VM agent.** Use [trycua/cua](https://github.com/trycua/cua).
- **Hosting the native Anthropic `computer_20251124` tool type.** That tool is API-specific and built into the Claude models; MCP cannot transport it. We mirror its action vocabulary where useful but do not pretend to be it.
- **Community polish.** First-pass MVP is for the author's personal use. Distro packaging, CI, multi-distro testing, and contributor onboarding come after the tool proves valuable.

---

## 4. Design philosophy and principles

These principles are load-bearing: they should be the first thing referenced when adding a new tool or response field. They are listed in order of "weight" — earlier ones outrank later ones when they conflict.

### 4.1 Accessibility-tree first; pixels as fallback

When a structured representation of the UI exists (AT-SPI for native apps, accessibility tree for Qt/GTK/Firefox/Electron), prefer it categorically over screenshots. Reasons: deterministic IDs (no coordinate hallucination), 5–30× better token efficiency, surviving small visual changes, supporting `do_action()` invocations that skip input emulation entirely.

The primary action tool is `act_on_element(element_ref, action)`. Coordinate-based tools (`mouse`, `screenshot`) exist as explicit fallbacks for canvas/video/PDF/Electron-without-a11y. When pixels are used, response fields mark `vision_fallback: true` so it is visible in the audit log and the agent's next-turn context.

### 4.2 Response-as-instruction

Tool responses are the freshest tokens in the agent's attention window. Tool descriptions live thousands of tokens away in context. Therefore: **encode behaviour-shaping information in structured response fields, not in prose at the top of the tool description.**

Examples baked into this design:
- Action tools return a `diff` field (`windows_added`, `windows_removed`, `focus_changed`, `cursor`) — the agent sees side effects without a follow-up call.
- `stale_handle` errors return `current_state` inline — no extra round-trip to re-observe.
- `wait` timeouts return `closest_match`, `current_windows`, `elapsed_ms`.
- `act_on_element` failures return `available_actions` and a `fallback_suggestion`.
- `screenshot` responses include the structural snapshot at capture time.
- Sensitive-path edits return `warnings: ["modified file under ~/.ssh/"]`.
- Virtual mode with no windows returns `hint: "virtual session has no running apps; use bash to launch one"`.

Tool descriptions become factual and short. The response shape carries the rest.

### 4.3 High signal-to-noise per token

Every field in every response must materially change the agent's next decision. If `windows_added: []` is empty 95% of the time, omit the key on empty rather than always returning it. Strip Wayland surface IDs, X11 backing IDs, KWin internal handles, MIME types, raw pre-scaled image dimensions, and full Qt class hierarchies. Keep `role`, `name`, `value`, `bbox`, `actions` for elements; `uuid`, `title`, `class`, `geometry`, `screen` for windows.

Source: Anthropic ["Writing tools for agents"](https://www.anthropic.com/engineering/writing-tools-for-agents), ["Effective context engineering"](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents). This is the master heuristic that bounds the others.

### 4.4 Progressive disclosure

Don't choose between full and summary — let the agent choose, per call. Default to cheap; expensive details are opt-in.

Applied: `observe(detail: "summary" | "interactive" | "full" = "interactive")`. `inspect(max_depth: int = 3)`. `window_query(format: "ids" | "titles" | "full" = "titles")`. `screenshot(max_long_edge: int = 1280)`.

### 4.5 Poka-yoke — make wrong actions structurally impossible

When a class of agent mistake recurs, change the tool's **shape** so the mistake cannot be expressed. The model can't reliably remember a "warning" in a description; it can't violate a schema.

Applied:
- `act_on_element` and `window_control` accept opaque refs (`element_id`, `window_uuid`) that must come from a recent `observe`/`inspect`/`window_query` within this MCP session. The server validates against an in-memory set with TTL. Fabricated/stale refs return `stale_handle` with fresh state inline. The model **cannot** pass a window title or coordinate to these tools — the signature does not accept them.
- `keyboard.type` accepts `text` only (no modifier param). `keyboard.key` accepts `keys` only (no text param). Cross-cutting mistake of "type while holding Ctrl" can only be expressed by separate sequenced calls, which is what we want.
- `text_editor_edit` requires absolute paths. Relative paths rejected at the schema layer.
- `mouse.click` allows raw coordinates — explicit escape hatch, marked `vision_fallback: true` in response.

### 4.6 Just-in-time references over pre-loaded payloads

Return references, let the agent fetch detail when needed. Don't pre-load expensive trees on the off chance they're needed.

Applied:
- `observe` returns the focused window's AT-SPI tree shallow; other windows return only `{ref, title, class}`.
- `screenshot` `region: "full"` returns a `resource_link` to a file under `$XDG_RUNTIME_DIR/kde-mcp/`; targeted screenshots (window/element) stay inline.
- `bash` output >16k chars returns a tail and a `full_output: resource_link` to the captured file.

### 4.7 Quarantine untrusted content

Any string the agent reads that came from outside its own authorship (window content, clipboard from elsewhere, file contents we didn't write, OCR text) is a potential prompt-injection vector. Mark it structurally so the model and downstream tools treat it as data, not instruction.

Applied: every response field carrying external text wears a `trust` discriminator with three levels:
- `trusted` — comes from OS APIs (window class, AT-SPI role enum, geometry, mode)
- `agent_authored` — content the agent recently wrote (clipboard we just set; file we just created)
- `external` — window titles, AT-SPI `name`/`value`/`description` fields, clipboard read we didn't write, OCR text, bash stdout, file contents read via `text_editor_view`

Concrete shape: `{ "title": { "trust": "external", "text": "Save File" }, "role": { "trust": "trusted", "text": "PushButton" } }`. Cost is ~10% token overhead; benefit is structural prompt-injection safety the agent's prompt cannot override.

### 4.8 Tensions and resolutions

Tensions between principles are real. The spec calls them out explicitly so contributors don't accidentally undo a decision.

| Tension | Resolution |
|---|---|
| Signal-to-noise (4.3) vs. response-as-instruction (4.2) | Only add a field if it materially changes the next action. Empty diffs and empty arrays are omitted, not returned as `[]`. |
| Poka-yoke / splitting (4.5) vs. tool consolidation | **Consolidate along intent, split along safety.** `act_on_element` is one tool (one intent). `type` and `key` are two tools (different blast radii). |
| Just-in-time refs (4.6) vs. response-as-instruction (4.2) | Return ref **plus** the meta that drove the call (title, role). Pure-ref responses are too sparse. `windows_added: [{ref, title}]` is the right shape; `[ref1, ref2]` is too sparse. |
| AT-SPI tree primacy (4.1) vs. quarantine untrusted (4.7) | The AT-SPI *structure* is trusted (roles, geometry, action names). The *strings* inside (name, value, description) are external. Mark per-field with `trust`. |

---

## 5. Architecture

### 5.1 Process model

One Rust binary, `kde-mcp`. The mode is chosen at process start via `--mode=live | --mode=virtual` and is **immutable for the life of the process**. There is no runtime mode switch.

Users who want both modes available simultaneously configure two MCP server entries in their host:

```jsonc
// ~/.config/Claude/claude_desktop_config.json (or equivalent)
{
  "mcpServers": {
    "kde-live":    { "command": "kde-mcp", "args": ["--mode=live"] },
    "kde-virtual": { "command": "kde-mcp", "args": ["--mode=virtual", "--vnc=5910"] }
  }
}
```

Two processes, two audit logs, two failure domains, no internal mode-routing plumbing. See ADR-0003 for the rationale.

### 5.2 Component diagram

```
┌──────────────────────────────────────────────────────────────────────┐
│  MCP host (Claude Code / Claude Desktop / Cline / Cursor / Zed)      │
└───────────────────────────────┬──────────────────────────────────────┘
                                │ stdio · JSON-RPC · MCP 2025-11-25
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  kde-mcp (single Rust binary, single tokio runtime)                  │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ rmcp server (tools/list, tools/call, content blocks)           │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  Tools (each module ≈ 1 file in src/tools/):                         │
│    observe · inspect · screenshot · read_text · wait                 │
│    act_on_element · mouse · keyboard                                 │
│    window_query · window_control                                     │
│    clipboard_read · clipboard_write                                  │
│    bash · text_editor_view · text_editor_edit                        │
│                                                                      │
│  Backend services:                                                   │
│    src/kwin/scripting.rs    — workspace queries via loadScript       │
│    src/kwin/screenshot.rs   — org.kde.KWin.ScreenShot2 client        │
│    src/kwin/eis.rs          — org.kde.KWin.EIS.RemoteDesktop → reis  │
│    src/atspi.rs             — connection, tree, element-ref encoding │
│    src/util/pty.rs          — portable-pty persistent bash session   │
│                                                                      │
│  Cross-cutting:                                                      │
│    src/audit.rs             — JSONL audit log writer                 │
│    src/refs.rs              — opaque ref encoding + TTL validation   │
│    src/trust.rs             — trust wrappers on external strings     │
│    src/mode/live.rs         — connect to user session                │
│    src/mode/virtual.rs      — spawn + bootstrap + teardown children  │
│    src/error.rs             — taxonomy → MCP isError responses       │
└───────────────────────┬──────────────────────────┬───────────────────┘
                        │                          │
         ┌──────────────▼───────────────┐  ┌───────▼──────────────────┐
         │ Live mode (--mode=live)      │  │ Virtual mode children:    │
         │ ───────────────────────────  │  │ ───────────────────────── │
         │ User session bus             │  │ dbus-run-session          │
         │   ├─ KWin (user's, real)     │  │  └─ private DBUS_SESSION  │
         │   ├─ User's AT-SPI bus       │  │      ├─ kwin_wayland      │
         │   └─ User's PipeWire         │  │      │   --virtual         │
         │                              │  │      ├─ at-spi-bus-launch │
         │                              │  │      └─ wayvnc :5910      │
         └──────────────────────────────┘  └───────────────────────────┘
                                                       │
                                                       │ VNC localhost:5910
                                              ┌────────▼────────┐
                                              │ KRDC (observe)  │
                                              └─────────────────┘
```

### 5.3 Live mode bootstrap

1. Parse CLI args + load config (TOML at `~/.config/kde-mcp/config.toml` if present).
2. Open audit log at `~/.local/state/kde-mcp/audit.jsonl` (create parent dirs).
3. Open `zbus::Connection::session()` against `$DBUS_SESSION_BUS_ADDRESS`.
4. Call `org.kde.KWin.EIS.RemoteDesktop` → receive libei fd → init `reis::Context`.
5. Open `atspi::AccessibilityConnection` against the user's a11y bus.
6. Start MCP stdio loop. Print one-line startup banner on stderr for the host's MCP log.

Any failure in 3–5 → exit non-zero with structured stderr message.

### 5.4 Virtual mode bootstrap

The order is significant; reordering breaks the chain.

1. Parse CLI + config + open audit log (same as live).
2. Choose a random Wayland socket name (`wayland-<8-hex>`) to avoid collisions per [kwin-mcp's pattern](https://github.com/isac322/kwin-mcp/blob/main/src/kwin_mcp/session.py).
3. Spawn `dbus-run-session -- sleep infinity` as a tracked child; capture chosen bus address from its stdout / env handoff.
4. Set `DBUS_SESSION_BUS_ADDRESS` for subsequent child spawns; leave the parent process's own env unchanged.
5. Spawn `kwin_wayland --virtual --width 1024 --height 768 --no-lockscreen --socket=<random>` as a child of (3).
6. Wait for the KWin D-Bus name to appear on the private bus (poll up to 3 s).
7. Bootstrap AT-SPI inside the private bus: parse `/usr/share/dbus-1/services/org.a11y.Bus.service` and `/usr/share/dbus-1/accessibility-services/org.a11y.atspi.Registry.service` to find the launcher binaries (handles Fedora's `/usr/libexec` vs Debian's `/usr/lib` divergence per [kwin-mcp issue #14](https://github.com/isac322/kwin-mcp/issues/14)). Spawn `at-spi-bus-launcher --launch-immediately` against the private bus; verify `org.a11y.Bus` registers; set `IsEnabled=true` on `org.a11y.Status`.
8. If `--vnc` set, spawn `wayvnc 127.0.0.1 <port>` bound to the virtual Wayland socket.
9. Open zbus connection to the private bus; call `org.kde.KWin.EIS.RemoteDesktop` on it; init reis context.
10. Open atspi connection to the private a11y bus.
11. Start MCP stdio loop. Stderr banner reports virtual session details (private bus, wayland socket, VNC port if set).

Hard timeout for the entire bootstrap: 5 seconds. Failure surfaces the stage of failure in the error message.

### 5.5 Teardown

On `SIGTERM`/`SIGINT`/stdin EOF:
1. Send SIGTERM to virtual-mode children in reverse spawn order; 2 s grace per child; SIGKILL after.
2. Clean up `$XDG_RUNTIME_DIR/wayland-<random>.lock` and the socket.
3. Flush audit log.
4. Exit 0.

### 5.6 In-process state

- One `tokio::runtime::Runtime` (multi-threaded).
- One `zbus::Connection` (whichever bus matches the mode).
- One `reis::Context` (libei fd handed over by KWin EIS at startup).
- One `atspi::AccessibilityConnection`.
- One lazily-spawned `PtySession` for `bash` (started on first call).
- An in-memory **valid-refs set** (`HashMap<RefId, RefMetadata>`) with TTL — the poka-yoke enforcement layer.
- An in-memory **window-list cache**, invalidated by KWin signals `windowAdded`/`windowRemoved`/`windowActivated`, with 1 s fallback TTL.
- No screenshot cache. Always fresh.

---

## 6. Tool catalogue

15 tools. Read-only set (8) auto-approves under Claude Code's default permission mode; destructive set (7) triggers approval prompts and additionally passes through the policy gate (§6.0).

All tools accept their parameters as the MCP `arguments` object; all return either:
- `{ content: [{ type: "text", text: "..." }, ...], structuredContent: {...}, isError: false }` on success
- `{ content: [...], structuredContent: { code, ...details }, isError: true }` on semantic failure

JSON-RPC protocol errors are reserved for malformed arguments only.

### 6.0 The policy gate (cross-cutting)

Every destructive tool call passes through a shared policy gate before any side effect is dispatched. The gate is implemented once (in `src/policy.rs`) and wraps each destructive handler:

1. **Lease acquisition** — if the tool is mutating (per ADR-0011's classification table), the process must acquire-or-refresh the lease at `$XDG_RUNTIME_DIR/kde-mcp/lease.json`. Acquire blocks up to `acquire_timeout` (default 60s) polling at 100ms. Failure → `busy { holder_pid, holder_intent, lease_expires_in_ms, waited_ms, acquire_timeout_ms }`. Observing tools refresh the lease only when the caller already holds it; they never block.
2. **Cancellation check** — if the global-shortcut-driven `CancellationToken` is set (user pressed `Meta+Esc`), abort immediately. Failure → `cancelled`. Token is rechecked between every libei event, every OCR page, every D-Bus call so long-running tools surrender quickly.
3. **Target-app resolution** — derives the target app class from tool args:
   - `act_on_element` / `window_control` → from the ref's owning window
   - `mouse` → KWin-script query "window at coordinate"
   - `keyboard` → currently-focused window's class
   - `bash` / `text_editor_edit` / `clipboard_write` → not per-app; gate steps 4–5 skipped
4. **Tier check** — table-driven (`~/.config/kde-mcp/policy.toml`). Three tiers:
   - `view_only` (browsers, trading platforms) — destructive tools rejected → `tier_restricted`
   - `click_only` (terminals, IDEs) — `keyboard.type` rejected; `keyboard.key` accepts only a navigation allowlist (`Tab`, `Return`, `Escape`, `Page_Up`/`Down`, `Home`/`End`, arrow keys, modifier combos like `Ctrl+C`/`Ctrl+V` allowed but not `Ctrl+Shift+T` / arbitrary chords); `mouse.click`/`scroll`/`drag` allowed
   - `full` (default) — no tier restriction
5. **Approval check** — first destructive tool targeting a not-yet-approved app:
   - Returns `isError: true, code: "app_approval_required", details: { app_class, app_title, requested_action, current_tier, sentinel_warning?, notification_id }`
   - In parallel, fires `org.freedesktop.Notifications.Notify` with actions `Allow once` / `Allow this session` / `Deny`. The notification's `app_id` is `kde-mcp` so it surfaces in KDE's notification UI consistently.
   - User picks → in-memory `HashMap<app_class, ApprovalState>` updated. Agent retries the tool; gate now passes.
   - If agent retries before user clicks: returns the same error with `still_pending: true`. Agent uses `wait { seconds: 2 }` between retries.
6. **Sentinel warning emission** — if the target app's tier table marks `sentinel: "shell_access" | "filesystem_access" | "system_settings"`, the response carries `warnings: ["target_app:konsole — equivalent to shell access"]`. Visible to the model and the user without blocking the action.

The gate is the only entry point to side effects. New destructive tools added later inherit all six steps for free.

### 6.1 Read-only tools (8)

#### `observe`
**Annotations:** `readOnlyHint: true`
**Description:** Situational snapshot of the desktop. No screenshot. Should be the agent's reflex first call when entering an unfamiliar state.
**Params:**
```
detail?: "summary" | "interactive" | "full" = "interactive"
include_other_windows?: bool = true     # in summary mode, just a count
```
**Returns** (`structuredContent`):
```jsonc
{
  "mode": "live" | "virtual",
  "cursor": [x, y],
  "focused_window": {
    "uuid": "win#3",                    // opaque ref, valid until invalidated
    "title": { "trust": "external", "text": "..." },
    "class": { "trust": "trusted", "text": "kate" },
    "pid": 1234,
    "geometry": [x, y, w, h],
    "screen": "DP-1",
    "desktop": 1,
    "minimized": false, "maximized": false, "fullscreen": false
  },
  "focused_element": {                  // null if no a11y or no focus
    "id": "el#a4f2",
    "role": { "trust": "trusted", "text": "EditableText" },
    "name": { "trust": "external", "text": "Document" },
    "value": { "trust": "external", "text": "..." } | null,
    "states": ["focused", "showing", "sensitive"],
    "bbox": [x, y, w, h],
    "actions": ["activate"]
  },
  "windows": [ {ref, title, class, geometry, screen} ],  // only in "interactive"/"full"
  "screens": [ {name, geometry, scale, primary} ],       // only in "full"
  "hint": "virtual session has no running apps; use bash to launch one"  // only when relevant
}
```

#### `inspect`
**Annotations:** `readOnlyHint: true`
**Description:** Drill into the AT-SPI tree of a specific window or element. Returns a flattened element list; tree topology is encoded by `parent_id` references.
**Params:**
```
target: "focused" | "window:<ref>" | "element:<ref>" = "focused"
max_depth?: int = 3                     # ≥ 0
role_filter?: string[]                  // e.g. ["PushButton", "EditableText"]
name_pattern?: string                   // regex
max_elements?: int = 200
```
**Returns** (`structuredContent`):
```jsonc
{
  "elements": [
    {
      "id": "el#...", "parent_id": "el#...", "child_count": 3,
      "role": {trust, text}, "name": {trust, text}, "value": {trust, text} | null,
      "states": [...], "bbox": [x,y,w,h],
      "actions": ["press", "activate"]
    }
  ],
  "truncated": false,                   // true if max_elements clipped the result
  "warnings": []                        // e.g. "AT-SPI tree empty — vision fallback recommended"
}
```

#### `screenshot`
**Annotations:** `readOnlyHint: true`
**Description:** Capture pixels. Prefer `observe`/`inspect` for structural questions. Use a targeted `region` (window or element) over `"full"` for token efficiency.
**Params:**
```
region?: "active_screen" | "full" | "window:<ref>" | "element:<ref>" | [x,y,w,h] = "active_screen"
include_cursor?: bool = true
max_long_edge?: int = 1280              # server-side pre-scale guard
```
**Returns:** image content block + `structuredContent`:
```jsonc
{
  "original_size": [w, h],
  "returned_size": [w, h],
  "scale_factor": 0.83,                 // multiply API coords by 1/scale_factor → screen coords
  "captured_state": { /* lite-observe snapshot at capture moment */ },
  "format": "png",
  "full_resource": "file://..."         // present only for region="full" (resource_link)
}
```

#### `read_text`
**Annotations:** `readOnlyHint: true`
**Description:** OCR a region. Use when AT-SPI doesn't expose text (canvas, PDF viewer, image). Output is always `trust: "external"`.
**Params:**
```
region: (same as screenshot)
lang?: string = "eng+rus"
include_bboxes?: bool = false
```
**Returns:** text content block (`trust: external`). With bboxes: `structuredContent.words: [{text, bbox, confidence}]`.

#### `wait`
**Annotations:** `readOnlyHint: true` (waiting itself changes nothing; what is being waited on is a separate tool call)
**Description:** Block until a structured condition is met. Replaces arbitrary sleeps.
**Params:**
```
condition: "seconds" | "screen_change" | "window_appears" | "window_disappears"
         | "element_appears" | "element_disappears" | "element_state"
timeout_ms?: int = 5000
seconds?: number                        # for "seconds"
region?: same as screenshot             # for "screen_change"
pixel_threshold?: float = 0.02
match?: { title_regex?, class?, pid? }  # window match criteria
element_match?: { role?, name_pattern?, in_window?: <ref> }
required_state?: string                 # for "element_state"
```
**Returns** (`structuredContent`):
```jsonc
{
  "matched": true,
  "elapsed_ms": 743,
  "matched_window": {ref, title, class},   // when applicable
  "matched_element": {id, role, name},     // when applicable
  // on timeout:
  "timed_out": true,
  "closest_match": { /* best near-miss */ },
  "current_windows": [ ... ]
}
```

#### `window_query`
**Annotations:** `readOnlyHint: true`
**Description:** List windows or fetch geometry. No state changes.
**Params:**
```
action: "list" | "get_geometry" | "find"
format?: "ids" | "titles" | "full" = "titles"   # progressive disclosure
match?: { title_regex?, class?, pid?, role? }    # for "find"
window_uuid?: <ref>                              # for "get_geometry"
```

#### `clipboard_read`
**Annotations:** `readOnlyHint: true`
**Description:** Read clipboard contents.
**Params:**
```
mime_type?: string = "text/plain"
encoding?: "utf8" | "base64" = "utf8"
```
**Returns:** content + `structuredContent.origin: "external" | "agent_authored"` so the agent knows whether to treat clipboard as untrusted.

#### `text_editor_view`
**Annotations:** `readOnlyHint: true`
**Description:** View a file or list a directory. Matches Anthropic `str_replace_based_edit_tool` view semantics (cat -n with 1-indexed line numbers; directories via `find -maxdepth 2`).
**Params:**
```
path: string (absolute)
view_range?: [int, int]                 # 1-indexed inclusive; -1 = EOF
```

### 6.2 Destructive tools (7)

#### `act_on_element`
**Annotations:** `destructiveHint: true`
**Description:** Invoke an AT-SPI action on an element directly. Skips input emulation when the element supports the requested action. The killer primitive.
**Params:**
```
element_id: <ref from recent observe/inspect>
action: "press" | "activate" | "set_value" | "select" | "expand" | "collapse" | ...
value?: string                          # for "set_value"
```
**Returns** (`structuredContent`):
```jsonc
{
  "ok": true,
  "diff": { /* see §7.1 */ }
}
// On failure:
{
  "isError": true,
  "code": "not_actionable",
  "requested_action": "press",
  "available_actions": ["activate", "select"],
  "fallback_suggestion": "mouse.click at element bbox"
}
// or:
{
  "isError": true,
  "code": "stale_handle",
  "current_state": { /* fresh observe payload */ }
}
```

#### `mouse`
**Annotations:** `destructiveHint: true`
**Description:** Coordinate-based input. Explicit visual fallback when AT-SPI doesn't cover the target. Tool response marks `vision_fallback: true` so the audit log can count its use.
**Params:**
```
action: "click" | "move" | "drag" | "scroll" | "down" | "up" | "hold"
coordinate?: [x, y]                     # required for click/move/drag/down/up
start_coordinate?: [x, y]               # for "drag" (end is `coordinate`)
button?: "left" | "right" | "middle" = "left"
count?: int = 1                         # 2 = double, 3 = triple
modifier?: ("shift" | "ctrl" | "alt" | "super")[]
scroll_direction?: "up" | "down" | "left" | "right"
scroll_amount?: int
duration_ms?: int                       # for "hold"
```
**Returns:** `{ ok, diff, vision_fallback: true, verification_suggested?: "<hint>" }`.

If the click coordinate is over a screen region with no AT-SPI element underneath at the time of the click, the response includes `verification_suggested: "no AT-SPI element under click coordinates — visual confirmation recommended"`. The agent chooses whether to follow up with `screenshot` based on that hint.

#### `keyboard`
**Annotations:** `destructiveHint: true`
**Description:** Type text or send key chords. Note: `type` and `key` are separated by signature (poka-yoke) — `type` cannot send modifiers, `key` cannot send arbitrary text. Type with modifiers requires sequencing.
**Params:**
```
action: "type" | "key" | "hold_key"
text?: string                           # for "type"
keys?: string                           # for "key"/"hold_key", xdotool syntax ("ctrl+shift+s", "Return", "Page_Down")
duration_ms?: int                       # for "hold_key"
use_clipboard?: "auto" | "always" | "never" = "auto"
                                        # auto = clipboard-paste if text > 32 chars OR contains non-ASCII
```
**Returns:** `{ ok, diff }`. Original clipboard is saved+restored when `use_clipboard` paths are taken.

#### `window_control`
**Annotations:** `destructiveHint: true`
**Description:** Mutate window state via KWin scripting.
**Params:**
```
action: "focus" | "move" | "resize" | "minimize" | "maximize" | "restore"
      | "close" | "set_above" | "set_fullscreen"
window_uuid: <ref>
geometry?: [x, y, w, h]                 # for move/resize
value?: bool                            # for set_above / set_fullscreen
```
**Returns:** `{ ok, diff }`.

#### `clipboard_write`
**Annotations:** `destructiveHint: true`
**Description:** Set clipboard contents.
**Params:**
```
content: string
mime_type?: string = "text/plain"
encoding?: "utf8" | "base64" = "utf8"
```

#### `bash`
**Annotations:** `destructiveHint: true`, `openWorldHint: true`
**Description:** Run a command in a persistent shell session (one PTY for the life of the MCP process, lazy-started on first call). Output capped at 16k chars with a `full_output` resource_link to the captured file.
**Params:**
```
command?: string                        # required unless restart
restart?: bool                          # tear down and re-spawn the PTY
timeout_ms?: int = 120000
```
**Returns:**
```jsonc
{
  "ok": true,
  "exit_code": 0,
  "stdout": { "trust": "external", "text": "..." },
  "stderr": { "trust": "external", "text": "..." },
  "truncated": false,
  "full_output": "file:///tmp/.../bash-out-<id>.txt",   // when truncated
  "warnings": []                                         // sensitive-path / blocklist near-misses
}
```

**Server-side blocklist** (enforced before exec, returns `permission_denied`):
- `rm -rf /` and any variant ending at root
- `rm -rf ~` / `rm -rf $HOME`
- `dd of=/dev/sd[a-z]*`
- `mkfs.*` against block devices
- Fork bombs: `:(){:|:&};:`
- `sudo` and `pkexec` unless `--allow-sudo` config knob is set

These rules are not security; they are oh-shit prevention. The audit log records the attempt.

#### `text_editor_edit`
**Annotations:** `destructiveHint: true`
**Description:** Create / replace-in / insert-into files. Matches Anthropic `str_replace_based_edit_tool` semantics for `create`, `str_replace`, `insert`. (No `undo_edit` — matches the newer Anthropic versions.)
**Params:**
```
command: "create" | "str_replace" | "insert"
path: string (absolute)
file_text?: string                      # for create
old_str?: string                        # for str_replace; must match exactly once
new_str?: string                        # for str_replace
insert_line?: int                       # for insert; 0 = top
insert_text?: string                    # for insert
```
**Returns:** snippet of changed lines (matching reference impl ±4 lines of context). `warnings: []` includes any sensitive-path matches:
- `~/.ssh/`, `~/.gnupg/`, `/etc/`, `~/.config/keepassxc/`, `~/.config/git/`, `~/.aws/`, `~/.kube/`

### 6.3 Tool count summary

| # | Tool | Type |
|---|------|------|
| 1 | `observe` | read-only |
| 2 | `inspect` | read-only |
| 3 | `screenshot` | read-only |
| 4 | `read_text` | read-only |
| 5 | `wait` | read-only |
| 6 | `window_query` | read-only |
| 7 | `clipboard_read` | read-only |
| 8 | `text_editor_view` | read-only |
| 9 | `act_on_element` | destructive |
| 10 | `mouse` | destructive |
| 11 | `keyboard` | destructive |
| 12 | `window_control` | destructive |
| 13 | `clipboard_write` | destructive |
| 14 | `bash` | destructive |
| 15 | `text_editor_edit` | destructive |

---

## 7. Data flow and patterns

### 7.1 The action-diff shape

All destructive UI tools (`act_on_element`, `mouse`, `keyboard`, `window_control`) capture a "lite-observe" snapshot before and after the action and return the structured diff:

```jsonc
{
  "ok": true,
  "diff": {
    "focused_window": { "before": "win#3", "after": "win#7" },  // omitted if unchanged
    "focused_element": { "before": "el#a4", "after": "el#b1" }, // omitted if unchanged
    "windows_added": [ {ref, title, class} ],                    // omitted if empty
    "windows_removed": [ {ref, title, class} ],                  // omitted if empty
    "cursor": [840, 520]                                         // always present
  }
}
```

A "lite-observe" snapshot is: cursor position, focused window uuid + title, focused element id + role + name, window count. ~5–15 ms per snapshot (D-Bus calls hit the in-memory cache).

### 7.2 A typical agent turn

User: "Save this file as 'notes.md' on the desktop."

```
1. Model → observe
   Server: KWin Scripting workspace.activeWindow + atspi focused element
   Returns: focused_window=Kate, focused_element={role: DocumentSource}
2. Model → keyboard {action:"key", keys:"ctrl+shift+s"}
   Server: reis.send_key(Ctrl Shift S)
   Returns: { ok, diff: { windows_added: [{ref: "win#9", title:"Save File"}] } }
3. Model → inspect {target: "window:win#9", role_filter: ["EditableText","PushButton"]}
   Server: atspi traverse, filter, return flat list
   Returns: elements: [..., {id:"el#47", role:"EditableText", name:"Name:"}, {id:"el#52", role:"PushButton", name:"Save"} ...]
4. Model → act_on_element {element_id:"el#47", action:"set_value", value:"notes.md"}
   Returns: { ok, diff: {focused_element: {before:"el#47", after:"el#47" (value changed)}} }
5. Model → act_on_element {element_id:"el#52", action:"press"}
   Returns: { ok, diff: { windows_removed: [{ref:"win#9", title:"Save File"}] } }
```

5 tool calls, 0 screenshots, 0 coordinate math.

### 7.3 The ref system

Element IDs and window UUIDs returned by tools are opaque strings (`win#<n>`, `el#<hex>`) — short for token efficiency, stable for the life of the MCP process, validated server-side against an in-memory set with TTL.

When a tool that takes a ref receives one that the server cannot resolve, it returns a structured `stale_handle` error with a fresh `observe` payload inline. The agent gets the new world in the same turn and continues without an extra round-trip.

### 7.4 Caching and invalidation

- **Window list cache:** rebuilt eagerly on KWin signals `windowAdded`, `windowRemoved`, `windowActivated`. TTL 1 s as fallback for missed signals.
- **Element-app-root cache:** maps `(window_uuid → atspi_app_root)`. Rebuilt on KWin window-added signal. Element refs themselves are pass-throughs to atspi object paths.
- **Screenshot cache:** none. Always fresh.
- **Bash session:** persistent for the life of the process; restart explicitly via `bash {restart: true}`.

---

## 8. Error taxonomy

Every error returns `{ isError: true, structuredContent: { code, ...details } }`. Models adapt better to typed codes than to free-text. JSON-RPC protocol errors are only used for malformed arguments.

| Code | When | Recovery hint included |
|------|------|------------------------|
| `stale_handle` | element/window ref no longer valid | `current_state` (fresh observe) |
| `not_actionable` | AT-SPI action returned false / element doesn't support requested action | `available_actions`, `fallback_suggestion` |
| `target_not_found` | match criteria didn't resolve | `closest_match` (best near-miss) |
| `permission_denied` | server blocklist or KWin refusal | `reason` |
| `tier_restricted` | target app's tier disallows this action (§6.0 step 4) | `app_class`, `current_tier`, `allowed_actions` |
| `app_approval_required` | target app not yet approved this session (§6.0 step 5) | `app_class`, `notification_id`, `still_pending?` |
| `cancelled` | user invoked the cancel shortcut (`Meta+Esc`) | `cancelled_at_stage` |
| `busy` | another `kde-mcp` session currently holds the mutation lease (ADR-0011) | `holder_pid`, `holder_intent`, `lease_expires_in_ms`, `waited_ms`, `acquire_timeout_ms` |
| `timeout` | `wait` deadline | `closest_match`, `current_windows` |
| `input_dispatch_failed` | libei call errored | retried once internally; if surfaced, `libei_version`, `kwin_version` in details |
| `external_command_failed` | tesseract/wl-copy/etc. nonzero exit | `stderr` |
| `bus_disconnected` | zbus connection lost | indicates the MCP process is unusable; host should restart it |
| `mode_mismatch_internal` | internal invariant violation | logged at ERROR; process exits — never returned to the model |

---

## 9. Audit log

`~/.local/state/kde-mcp/audit.jsonl`, one JSON object per line. Rotated at 50 MB; keep 5.

```jsonc
{"ts":"2026-05-13T14:23:45.123Z","mode":"live","tool":"keyboard","args":{"action":"key","keys":"ctrl+shift+s"},"ok":true,"duration_ms":12}
{"ts":"...","mode":"live","tool":"bash","args":{"command":"ls ~/Documents"},"ok":true,"duration_ms":34}
{"ts":"...","mode":"live","tool":"text_editor_edit","args":{"command":"str_replace","path":"~/notes.md","old_str_hash":"sha256:...","new_str_hash":"sha256:..."},"ok":true,"duration_ms":3}
{"ts":"...","mode":"live","tool":"bash","args":{"command":"rm -rf /"},"ok":false,"error":{"code":"permission_denied","reason":"blocklist:rm -rf /"}}
{"ts":"...","mode":"live","tool":"mouse","args":{"action":"click","coordinate":[450,320]},"ok":true,"vision_fallback":true,"duration_ms":18}
```

**Redaction:**
- `bash.command` verbatim — we want this visible.
- `text_editor_edit` file content / old_str / new_str hashed if longer than 500 chars (log bloat, sensitive content).
- `clipboard_write.content` hashed always.
- Everything else verbatim.

The audit log is local-only. No telemetry, no phone-home, no opentelemetry endpoint in MVP.

---

## 10. Safety model

The MCP server cooperates with the host's permission mode (Claude Code's `default` / `acceptEdits` / `bypassPermissions` / `plan`) via correct annotations. **In addition**, it implements its own gate (§6.0) inspired by Anthropic's `computer-use` safety architecture; the two layers compose.

Eight layers:

1. **Honest tool annotations.** `readOnlyHint: true` on the 8 observation tools; `destructiveHint: true` on the 7 mutating tools. `openWorldHint: true` on `bash`. Hosts use these to decide which to auto-allow vs. prompt.
2. **Audit log always on.** Local JSONL; `tail -f` for live review. Every gate decision (approval grant, tier rejection, cancellation) is also logged.
3. **Per-session mutation lease.** `$XDG_RUNTIME_DIR/kde-mcp/lease.json` (POSIX flock on read/write). Multiple `kde-mcp --mode=live` may run concurrently — one per Claude Code session. Mutating tool calls serialize via the lease (acquire on dispatch, refresh on each subsequent call by the same holder, auto-expire after `idle_timeout` of inactivity, release on process exit / cancel). Observation calls are never blocked. Stale holders detected via `kill -0`. Notifications via `org.freedesktop.Notifications` on first acquire ("Claude is using your computer · press Meta+Esc to stop") and on clean shutdown ("Claude finished using your computer"). See `docs/adr/0011-multi-session-lease.md`.
4. **Per-app approval per session** (gate step 5). First destructive tool against a not-yet-approved app returns `app_approval_required` and fires a notification with `Allow once` / `Allow this session` / `Deny` action buttons. State is in-memory, lost on process restart.
5. **App-category tiering** (gate step 4). `view_only` (browsers): destructive tools blocked. `click_only` (terminals, IDEs): typing blocked, click/navigation allowed. `full`: default. Table at `~/.config/kde-mcp/policy.toml`, user-overridable. Tier rejection returns `tier_restricted`.
6. **Sentinel warnings** (gate step 6). When a destructive action targets an app marked with a sentinel (`shell_access`, `filesystem_access`, `system_settings`), the response carries `warnings: [...]`. Non-blocking; informational for both the model and the user.
7. **Terminal-excluded screenshots.** When `screenshot` is invoked, terminal-class windows (`konsole`, `yakuake`, `alacritty`, `kitty`, `wezterm`, `foot`, `ghostty`, `tilix`, `gnome-terminal`, `xterm`, `xfce4-terminal`, `ptyxis`, `warp-terminal`, …) are detected via KWin Scripting and their geometry is overwritten with a solid black rectangle + `[terminal masked]` label before the PNG is returned. Defense against on-screen content in the agent's own terminal feeding back as prompt injection. Config-overridable per class.
8. **Server-side hard blocklist for `bash`.** Cannot be disabled via MCP arguments (only via launch-time config). Patterns: `rm -rf /`, `rm -rf ~`, `dd of=/dev/sd*`, `mkfs.*`, `:(){:|:&};:`, raw `sudo`/`pkexec` (unless `--allow-sudo` config).
9. **Sensitive-path warnings (not blocks).** `text_editor_edit` and `bash` responses include `warnings: [...]` when touching `~/.ssh/`, `~/.gnupg/`, `/etc/`, `~/.config/keepassxc/`, `~/.config/git/`, `~/.aws/`, `~/.kube/`. The host's permission mode decides whether to escalate.
10. **Trust wrappers on external strings.** Any model-readable text from window content / clipboard read / OCR / bash stdout / file content carries `trust: "external"`. The agent sees data marked as data, not as instructions. Structural defense that the agent cannot override.
11. **Global cancel shortcut.** `Meta+Esc` (default; configurable) registered via KDE's KGlobalAccel D-Bus interface. KWin grabs the key at the compositor level — apps don't see it (so prompt injection cannot dismiss it). On press, every in-flight tool aborts at the next safe point and returns `cancelled`. A KDE notification appears: "Claude aborted by user."

**No mid-call `elicitation/create` prompts** in MVP — they're inconsistently supported by clients and would override the user's chosen permission mode. The per-app approval notification (#4) replaces this with an OS-level UI that integrates with KDE's notification flow.

### What we explicitly do not have

| Anthropic feature | Our substitute | Rationale |
|---|---|---|
| Server-side prompt-injection classifier | Trust wrappers (#10) + sensitive-path warnings (#9) + per-app approval (#4) | We lack classifier weights and training data; cannot reproduce |
| Apps hidden while Claude works | Per-app approval (#4) + tier restriction (#5) | Requires host-level turn awareness we don't have as an external MCP server; substitute restricts what Claude can *touch* even if it can *see* other apps |
| Bundled-with-host install | `.mcpb` extension bundle + `claude plugin install` registration | We're not Anthropic; one-command install is the closest we can get |

---

## 11. Testing strategy

**Unit tests** (`cargo test`, fast, no D-Bus): argument parsing, audit redaction, image-resize math, error taxonomy mapping, ref encoding, trust wrapper construction, blocklist regex matching.

**Integration tests** (require a Linux+KDE host or CI container with a virtual KWin):
- Test harness spawns a `--mode=virtual` server inside its own `dbus-run-session + kwin_wayland --virtual`.
- Test launches a known app inside the virtual session (`kate`, `kcalc`, `gnome-text-editor`).
- Test runs MCP tool calls via the same rmcp loop the real binary uses.
- Asserts on `structuredContent`.

Per-tool coverage target: at least one happy-path test against a real app + one stale-handle / error-path test.

Test apps to cover variance:
- KDE/Qt: `kate`, `kcalc`, `dolphin`
- GTK4: `gnome-text-editor`
- Firefox-ESR (AT-SPI variance)
- Electron via VS Code (a11y-degraded path; verifies `vision_fallback`)

**Manual smoke tests** (things automation cannot easily assert):
- Cursor visibility in screenshots
- Fractional-scaling coordinate accuracy on mixed-DPI multi-monitor (user runs 100%/150% setups — exercises this immediately)
- Live-mode behaviour on user's real desktop without breaking the user's running session

**No CI in MVP.** GitHub Actions matrix for the community phase later.

---

## 12. Installation

**System dependencies (Arch):**
```
sudo pacman -S libei at-spi2-core wl-clipboard wayvnc tesseract tesseract-data-eng \
               spectacle xdg-desktop-portal-kde
```

Fedora: same package names. Debian/Ubuntu: `apt install libei-bin at-spi2-core wl-clipboard wayvnc tesseract-ocr kde-spectacle xdg-desktop-portal-kde`.

**Build:**
```
cargo install --path . --root ~/.local
# Binary at ~/.local/bin/kde-mcp
```

**Authorize ScreenShot2 D-Bus interface:**
```
install -m 644 packaging/kde-mcp.desktop ~/.local/share/applications/
```
The `.desktop` file carries `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`, which KWin's `KApplicationTrader` check requires before honouring screenshot calls.

**Register with Claude Code:**
```
claude mcp add kde-live    ~/.local/bin/kde-mcp -- --mode=live
claude mcp add kde-virtual ~/.local/bin/kde-mcp -- --mode=virtual --vnc=5910
```

---

## 13. Project layout

```
kde-mcp/
├── Cargo.toml
├── README.md
├── LICENSE
├── .gitignore
├── src/
│   ├── main.rs                   # CLI parse, mode bootstrap, tokio init, rmcp serve
│   ├── lib.rs                    # module declarations
│   ├── config.rs                 # CLI + ~/.config/kde-mcp/config.toml
│   ├── audit.rs                  # JSONL audit logger (also logs gate decisions)
│   ├── error.rs                  # Error taxonomy → MCP responses
│   ├── refs.rs                   # opaque ref encoding + TTL validation set
│   ├── trust.rs                  # trust wrappers on external strings
│   ├── policy.rs                 # the policy gate (§6.0): lock + cancel + tier + approval + sentinel
│   ├── lock.rs                   # single-session lockfile per mode
│   ├── notify.rs                 # org.freedesktop.Notifications client (approval prompts + lock events)
│   ├── cancel.rs                 # KGlobalAccel registration + CancellationToken broadcast
│   ├── mode/
│   │   ├── mod.rs
│   │   ├── live.rs               # connect to user session
│   │   └── virtual.rs            # spawn + bootstrap children + cleanup
│   ├── kwin/
│   │   ├── mod.rs
│   │   ├── scripting.rs          # workspace queries via loadScript
│   │   ├── screenshot.rs         # org.kde.KWin.ScreenShot2 client
│   │   └── eis.rs                # org.kde.KWin.EIS.RemoteDesktop → reis::Context
│   ├── atspi/
│   │   ├── mod.rs
│   │   ├── connection.rs
│   │   ├── tree.rs               # traversal, filter, flatten
│   │   └── ref_encoding.rs       # element ref ↔ atspi object path
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── observe.rs
│   │   ├── inspect.rs
│   │   ├── screenshot.rs
│   │   ├── read_text.rs
│   │   ├── act_on_element.rs
│   │   ├── mouse.rs
│   │   ├── keyboard.rs
│   │   ├── window.rs             # window_query + window_control
│   │   ├── clipboard.rs          # clipboard_read + clipboard_write
│   │   ├── wait.rs
│   │   ├── bash.rs
│   │   └── text_editor.rs        # text_editor_view + text_editor_edit
│   └── util/
│       ├── mod.rs
│       ├── image.rs              # ARGB→PNG, resize, scale-factor math, terminal-mask overlay
│       └── pty.rs                # persistent bash session
├── packaging/
│   ├── kde-mcp.desktop
│   └── README-install.md
├── docs/
│   ├── specs/
│   │   └── 2026-05-13-kde-computer-use-mcp-design.md   (this file)
│   ├── research/
│   │   ├── anthropic-computer-use-reference.md
│   │   ├── wayland-kde-primitives.md
│   │   ├── prior-art.md
│   │   └── llm-tool-design-principles.md
│   ├── adr/
│   │   ├── README.md
│   │   ├── 0001-language-rust.md
│   │   ├── 0002-kde-only-scope.md
│   │   ├── 0003-mode-per-process-no-runtime-swap.md
│   │   ├── 0004-response-as-instruction.md
│   │   ├── 0005-accessibility-tree-first.md
│   │   └── 0006-tool-surface-shape.md
│   └── comparison-with-anthropic-computer-use.md
└── tests/
    ├── integration/
    └── apps/
```

### 13.1 Library version pins and plan-level adaptations (added 2026-05-14)

The pinned versions in HANDOFF.md (`rmcp = "0.1"`, `reis = "0.4"`, `atspi = "0.22"`) were placeholders captured during the design conversation. Before plan writing, current crates.io versions were surveyed; see `docs/research/2026-05-13-rust-stack-current-apis.md` for the full delta. The pins below are what Phase 0 will compile against.

| Crate | Pin | Notes |
|---|---|---|
| `rmcp` | `1.7.0` | Major rewrite vs. 0.1. Uses `#[tool_router]` / `#[tool]` / `#[tool_handler]` proc-macros. See "Module-layout adaptation" below. |
| `atspi` | `0.30.0` (tokio feature) | Pre-1.0, expect quarterly bumps. Pin exactly. |
| `reis` | `=0.6.1` | Single-maintainer, explicit "API may change" notice. Pin exactly; budget one bring-up break per year. |
| `zbus` | `5.15.0` | |
| `tokio` | `1.52` (current at design time; floor `1.40`) | multi-thread runtime, default features. |
| `notify-rust` | `4.17.0` | `org.freedesktop.Notifications` client; action-button delivery is the load-bearing surface. |
| `portable-pty` | `0.9.0` | persistent bash session. |
| `fd-lock` | `4.0.4` | single-session lockfile. |
| `image` | `0.25.10` | ARGB → PNG, resize, terminal-mask overlay. |
| `tesseract` | `0.15.2` (binding) | OCR. `leptess 0.14` is an alternative if `tesseract` proves brittle. |
| `thiserror` | `2.0.x` | library error types. |
| `anyhow` | `1.0.x` | binary-boundary error. |
| `clap` | `4.6.x` | CLI parsing. |
| `serde` / `serde_json` | `1.0.x` | wire format. |
| `toml` | `1.1.x` | `~/.config/kde-mcp/policy.toml`. |
| `tracing` / `tracing-subscriber` | `0.1.44` / `0.3.23` | structured logging. |
| `xkbcommon` | `0.8.x` (added 2026-05-14) | **Not previously listed.** Required for evdev-minus-8 keycode conversion when injecting input through `reis`. Surfaces in `src/tools/keyboard.rs`. |

**MSRV:** Rust 1.85+ (edition 2024). Local development against stable 1.95+. Spec §13 originally said "1.78+"; the higher floor unlocks stable async-fn-in-traits and current edition without affecting any decision in ADRs 0001–0007.

**Module-layout adaptation (rmcp 1.7).** The §13 tree above sketches one tool implementation per `src/tools/<name>.rs`. That maps poorly onto the current `rmcp` API, which concentrates tool routing in a single struct with `#[tool_router]` + `#[tool]` methods. Resolution:

- A new `src/server.rs` holds the `KdeMcp` struct with all `#[tool]` methods (15 of them). Each `#[tool]` method is a thin shim: parameter validation, then delegation to a helper module under `src/tools/`.
- The `src/tools/<name>.rs` files survive — they expose **free functions** (`pub(crate) async fn observe(state: &KdeMcp, args: ObserveArgs) -> Result<ObserveResponse, ToolError>`) called by the matching `#[tool]` shim. Same code, same testability, different binding to the MCP wire layer.
- `src/lib.rs` adds `pub mod server;` and re-exports the `KdeMcp` type for the integration tests.

This is a plan-level adaptation, not a design change. ADR-0006 (15 tools, split read/write, consolidated along intent) is unchanged — the proc-macro form is just how `rmcp 1.7` expresses it.

**Phase 0 spike tasks (verify before any tool code).** These come out of the API research and must pass in Phase 0 before Phase 1 starts:

1. Confirm `rmcp::ProtocolVersion::V_2025_11_25` (or whichever variant covers the latest spec) is exported. If only `V_2024_11_05` is available, the conformance claim in §1 is provisional — the server still works, but the citation in §16 needs to match what `rmcp` actually advertises.
2. Verify `#[tool_router]` composes across multiple `impl` blocks (so `src/server.rs` can split tool methods by category without forcing a 15-method monolith).
3. Read `atspi-proxies/src/action.rs` to nail the `do_action` signature used by `act_on_element`.
4. Read `reis::pointer_absolute` (exact module / fn name unverified) for `motion_absolute`. If the API takes a different shape, `src/tools/mouse.rs`'s `move_to` semantics need adjusting.
5. Verify KWin's `org.kde.KWin.EIS.RemoteDesktop` D-Bus method signature against the live system with `qdbus6` before writing `src/kwin/eis.rs`.

These are scope-bounded, falsifiable, and short. Each becomes one task in the Phase 0 plan.

---

## 14. MVP phasing

Each phase ends with a usable binary.

| Phase | Scope | Est. days |
|-------|-------|-----------|
| 0 | Skeleton: `cargo new`, rmcp wiring, `--mode` flag, audit log, error taxonomy, single-session lock + KDE notifications on acquire/release, all 15 tools stubbed to return `not_implemented`. Verifies the binary appears in Claude Code's MCP list with the right tool inventory. | 1.5 |
| 1 | Read-only against live: `observe`, `inspect`, `window_query`, `screenshot` (with terminal-excluded masking), `clipboard_read`, `text_editor_view`. Highest-value subset for "look at my screen and tell me what's wrong" workflows. | 4–5 |
| 2 | Input against live: `mouse`, `keyboard`, `clipboard_write`, `act_on_element`. Wire reis to KWin EIS fd. Smoke test: agent does "open kcalc, type 2+3, press =, read the result." | 3–4 |
| 3 | **Safety policy gate (§6.0)**: `src/policy.rs`, tier table at `~/.config/kde-mcp/policy.toml`, sentinel warnings, per-app approval via `org.freedesktop.Notifications` action buttons, `Meta+Esc` global shortcut via KGlobalAccel + `CancellationToken` plumbing through all destructive tools. | 4–5 |
| 4 | Shell + edit + wait + OCR: `bash`, `text_editor_edit`, `wait`, `read_text`, `window_control`. Each goes through the policy gate where applicable. Anthropic-quickstart-equivalent parity reached for live mode. | 2–3 |
| 5 | Virtual mode: bootstrap of `dbus-run-session` + `kwin_wayland --virtual` + `at-spi-bus-launcher` + `wayvnc`. All tools work against virtual via the same code paths (the policy gate applies identically). Distro-specific AT-SPI bootstrap (kwin-mcp issue #14 lesson). | 3–5 |
| 6 | Polish: sensitive-path warnings tuning, blocklist refinement, trust-wrapper coverage audit, tool-description tuning, audit-log rotation, signal-handling cleanup, policy.toml example with sensible defaults, install scripts (`.mcpb` packaging research). | open-ended |

Total realistic: 5–7 calendar weeks for a single developer at part-time pace. Day-count: 18–23 working days for phases 0–5, plus polish.

---

## 15. Open questions / future work

Things the design deliberately does not commit to in MVP:

- **Journey tools** (`save_current_document()`, `open_app_and_wait_until_ready()`). Worth considering once primitives are exercised and repeating sequences are observed. Premature now.
- **CLI+Skills delivery** alongside MCP. Microsoft's Playwright team has recently argued CLI+Skill is more token-efficient than MCP for some workflows. Out of scope for v1 (user explicitly chose MCP), but a v2 question.
- **Streamable-HTTP transport** for remote driving. ~500 LOC; deferred until a real use case appears.
- **Elicitation prompts** for destructive actions. Replaced by the per-app approval flow via `org.freedesktop.Notifications` (§10 layer 4) which integrates with KDE's native UI. The MCP `elicitation/create` path may be revisited when client support stabilises, as a host-fallback when the user isn't on KDE notifications.
- **Persistent approval state across sessions.** Currently per-process / per-session. A `~/.local/state/kde-mcp/approvals.json` with TTL could persist "Allow this app forever" decisions. Out of MVP scope.
- **App-category tier auto-discovery.** The `policy.toml` table is hand-maintained for MVP. Eventually we could heuristically tier unknown apps (any app whose class matches `*terminal*` or `*-shell` → click_only, etc.) or pull from a community-maintained list.
- **Multi-compositor.** GNOME via Mutter portal-based RemoteDesktop, wlroots via virtual-pointer/virtual-keyboard. Each adds substantial backend code; out of scope for v1.
- **Server-mode authentication** for streamable-HTTP (if ever added).

---

## 16. References

Anthropic:
- [Computer Use Tool](https://docs.claude.com/en/docs/build-with-claude/computer-use)
- [Reference impl: anthropics/anthropic-quickstarts/computer-use-demo](https://github.com/anthropics/anthropic-quickstarts/tree/main/computer-use-demo)
- [Writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [Building effective agents](https://www.anthropic.com/engineering/building-effective-agents)

MCP:
- [Spec 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [Rust SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk)

KDE / Wayland:
- [KWin Scripting API](https://develop.kde.org/docs/plasma/kwin/api/)
- [KDE — Configuring Portal Permissions in Plasma](https://planet.kde.org/david-redondo-2025-09-04-configuring-portal-permissions-in-plasma/)
- [KWin MR !5496 — libeis backend](https://invent.kde.org/plasma/kwin/-/merge_requests/5496)
- [KWin MR !759 — ScreenShot2 D-Bus interface v2](https://invent.kde.org/plasma/kwin/-/merge_requests/759)
- [libei + libeis API docs](https://libinput.pages.freedesktop.org/libei/)
- [reis crate (Rust libei bindings)](https://docs.rs/reis/)

Prior art:
- [isac322/kwin-mcp](https://github.com/isac322/kwin-mcp)
- [kurojs/wayland-mcp](https://github.com/kurojs/wayland-mcp)
- [microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp)
- [trycua/cua](https://github.com/trycua/cua)
- [wonderwhy-er/DesktopCommanderMCP](https://github.com/wonderwhy-er/DesktopCommanderMCP)

Tool design:
- [pgEdge — Lessons writing an MCP server for PostgreSQL](https://www.pgedge.com/blog/lessons-learned-writing-an-mcp-server-for-postgresql)
- [PagerDuty MCP retro](https://www.pagerduty.com/eng/lessons-learned-while-building-pagerduty-mcp-server/)
- [Matt Adams — MCP Server Design Principles](https://www.matt-adams.co.uk/2025/08/30/mcp-design-principles.html)
- [Simon Willison — Agents Rule of Two](https://simonw.substack.com/p/new-prompt-injection-papers-agents)
