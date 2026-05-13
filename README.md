# kde-mcp

> An MCP server that gives any AI agent the ability to observe and control a KDE Plasma 6 / Wayland desktop — through programmatic primitives (AT-SPI, KWin Scripting, libei via KWin's private EIS interface) first, screenshots only as a fallback.

**Status:** design / specification phase. No code yet.
**Target:** KDE Plasma 6 on Wayland, KWin. Single-user, personal use first.
**Language:** Rust.
**License:** MIT.

## What this is

The Linux/KDE equivalent of the `computer-use` MCP server Anthropic ships built into Claude Code on macOS (and Claude Desktop on macOS + Windows). Anthropic's version is research-preview + macOS-only. This project covers KDE Plasma 6 Wayland, the user's daily-driver platform.

Distinguishing properties:

- **Native to KDE/Wayland.** Uses KDE's first-party D-Bus APIs (`org.kde.KWin.EIS.RemoteDesktop` for input, `org.kde.KWin.ScreenShot2` for capture, `org.kde.KWin.Scripting` for window control). No Xvfb, no Docker, no portal dialogs.
- **Accessibility-tree first.** Pixels are an explicit fallback. Most actions are deterministic AT-SPI invocations, not coordinate clicks. Inspired by the Playwright-MCP philosophy.
- **Response-shaped behavior.** Tool responses carry the structured deltas that drive the agent's next decision; prompt-engineering instructions are minimised.
- **Two-process model.** `kde-mcp --mode=live` controls your real desktop. `kde-mcp --mode=virtual` spawns a headless `kwin_wayland --virtual` session (optionally with `wayvnc` so you can watch over VNC) for isolated automation. No runtime swap, no single-process dual-context fragility.
- **Safety architecture borrowed from Anthropic's design** (ADR-0007): single-session lock with KDE notifications, per-app approval per session via `org.freedesktop.Notifications`, app-category tiering (view-only browsers, click-only terminals/IDEs, full default), sentinel warnings for high-impact apps, terminal-excluded screenshots, `Meta+Esc` global cancel via KGlobalAccel. Three Anthropic features we explicitly cannot reproduce: server-side classifier, host-level app-hiding, bundled-with-host install — see comparison doc for substitutes.

## Why not just use Anthropic's `computer-use` / kwin-mcp?

Anthropic's `computer-use` is **macOS-only** (research preview, requires Pro/Max). On Linux you have:

| Concern | Anthropic `computer-use` (macOS) | kwin-mcp | kde-mcp (this project) |
|---|---|---|---|
| Available on Linux | ❌ | ✅ | ✅ |
| Compositor | macOS | Wayland (KDE) | Wayland (KDE) |
| Input mechanism | macOS Accessibility API | libei via KDE EIS | libei via KDE EIS |
| Primary surface | screenshots | mixed | a11y tree first; pixels fallback |
| Response shape | screenshot after every action | mostly screenshots | structured diffs, screenshots opt-in |
| Tool count (estimated) | ~5 | 30 | 15 |
| Mode model | live only | live + virtual in one process | one mode per process |
| Distribution | bundled with Claude Code CLI | Python package | single Rust binary (.mcpb planned) |
| Per-app approval | terminal prompt | none | KDE notification with action buttons |
| App-category tiering | yes | none | yes |
| Global cancel shortcut | Esc (key consumed) | none | Meta+Esc (key consumed) |
| Server-side injection classifier | yes | none | none (structural substitutes) |
| Apps hidden during turn | yes | none | none (substitute: per-app approval + tier) |
| Language | not public | Python | Rust |

For the full side-by-side comparison see `docs/comparison-with-anthropic-computer-use.md`. For the design rationale see `docs/specs/2026-05-13-kde-computer-use-mcp-design.md`.

## Quick orientation

- `CLAUDE.md` — operating instructions for AI assistants working in this repo (read first if you're a fresh Claude session)
- `docs/HANDOFF.md` — current project state and the concrete next step (read second)
- `docs/specs/` — the design specification (the load-bearing document)
- `docs/research/` — collated research that informed the design (Anthropic spec deep-dive, Wayland/KDE primitive landscape, prior-art survey, LLM tool-design principles)
- `docs/adr/` — Architecture Decision Records (7 of them): one file per non-obvious choice with the reasoning frozen at the moment it was made
- `docs/comparison-with-anthropic-computer-use.md` — side-by-side analysis
- `packaging/` — `.desktop` file for KWin ScreenShot2 D-Bus authorization, install notes, and `policy.toml.example` with sensible safety defaults
- `src/` — Rust source (empty until implementation phase begins)

## Hard scope limits

- Not multi-compositor. KDE only. (GNOME/wlroots are tractable later but not in this design.)
- Not multi-platform. Linux only.
- Not a browser-automation tool — use Playwright-MCP for browsers.
- Not a sandboxed-VM agent — use Cua for that.
- Not headed for community use *yet*. Personal-use MVP first; community polish later if the tool proves useful.

## Status of the work

This repository currently contains design artifacts only. **Implementation has not started.** The brainstorming workflow that produced this design ends here; the next step is invoking the `superpowers:writing-plans` skill to turn the spec into a sequenced implementation plan (see `docs/HANDOFF.md`).

To continue work in a fresh session, open this repo and read `CLAUDE.md` and `docs/HANDOFF.md` first.
