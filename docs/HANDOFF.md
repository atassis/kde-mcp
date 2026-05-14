# Handoff — pick up where we left off

A snapshot of project state at the moment the design conversation paused. Read this first if you are returning to the project (Claude session or otherwise) after time away.

---

## Current status (2026-05-14)

- **Phase:** **Phase 0 complete.** Ready for Phase 1 (read-only tools).
- **Last commit:** `1c82d6d style(phase-0): cargo fmt + clippy clean` (Phase 0 sign-off).
- **Tag:** `phase-0` (local; not pushed — there's still no remote).
- **Branch:** `main` (single branch; no remote configured).
- **Working tree:** clean.

### What Phase 0 shipped

The `kde-mcp` binary compiles, registers as an MCP server, and advertises 16 wire-named tools (15 logical — `keyboard.{type,key}` and `text_editor.{view,edit}` split). Every tool returns `not_implemented` with the structured `{code, details}` payload per spec §8.

End-to-end smoke-verified on the dev machine (CachyOS, KWin 6 Wayland) via a scripted JSON-RPC handshake (`tests/integration/SMOKE.md`):

- `initialize` round-trips with `protocolVersion: 2025-11-25`, `serverInfo: {name: 'kde-mcp', version: '0.1.0'}`.
- `tools/list` returns the 16-name set with correct schemas + `readOnlyHint`/`destructiveHint`/`openWorldHint` annotations.
- `tools/call` returns `isError: true` with the structured payload.
- Per-mode lockfile acquired at `$XDG_RUNTIME_DIR/kde-mcp/<mode>.lock`; cross-process contention reports holder PID.
- KDE acquire / release notifications fire on startup / shutdown.
- 22 unit tests + 15 integration tests pass under `cargo test --workspace`.
- `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Phase 0 also produced a non-trivial bonus

`examples/eis-keystroke-prober/` is a standalone POC that exercises the **exact code path Phase 2 will use** for libei keystroke injection: zbus → `org.kde.KWin.EIS.RemoteDesktop.connectToEIS(1)` → reis `handshake_tokio(Sender)` → virtual keyboard device → `Ctrl+Tab`. Verified working live against Brave; findings are recorded in `docs/research/2026-05-14-phase-0-spikes.md` postscript. **This de-risks Phase 2 significantly** — the previously-unverified primitive is now known to work on this exact stack.

## What was decided

The spec at `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` is the source of truth. The seven ADRs at `docs/adr/0001-…` through `0007-…` record the load-bearing decisions individually. The comparison doc at `docs/comparison-with-anthropic-computer-use.md` puts kde-mcp side by side with Anthropic's macOS-only `computer-use` for context.

The headline decisions (each backed by an ADR):

| What | ADR | Why |
|---|---|---|
| Rust, not Python or TypeScript | 0001 | Static binary, lower footprint, stronger D-Bus / libei / AT-SPI bindings |
| KDE Plasma 6 / Wayland only | 0002 | Personal-use, KDE has the cleanest no-portal-dialog path; multi-compositor cost not worth it for MVP |
| Mode-per-process (no runtime swap) | 0003 | Avoid silent misrouting in dual-context state; fault isolation at OS process boundary |
| Response-as-instruction principle | 0004 | Response fields shape behaviour more reliably than prose in tool descriptions |
| Accessibility-tree first; pixels fallback | 0005 | Deterministic, 5-30× cheaper than screenshots; Playwright-MCP's lesson |
| 15 tools split read/write | 0006 | Consolidate along intent; split along safety for clean per-tool permissioning |
| Adopt Anthropic safety architecture (policy gate) | 0007 | Lock + tier + per-app approval + sentinel + terminal-mask + Meta+Esc cancel |

The MVP is sized at **18–23 working days for phases 0–5**, plus open-ended polish. Realistic 5–7 calendar weeks at part-time pace.

## What is NOT yet done

In strict order:

1. ~~Spec review by atassis.~~ Done 2026-05-13.
2. ~~Invocation of `superpowers:writing-plans`.~~ Done; plan at `docs/plans/2026-05-14-phase-0-skeleton.md`.
3. ~~**Phase 0 implementation.**~~ **Done 2026-05-14.** See "What Phase 0 shipped" above.
4. **Phase 1 plan + implementation:** read-only tools — `observe`, `inspect`, `window_query`, `screenshot` (with terminal masking), `clipboard_read`, `text_editor.view`. Spec §14 row 1. **Next step:** invoke `superpowers:writing-plans` again, this time targeting Phase 1.
5. **Phases 2–5** per spec §14.
6. **Phase 6 polish.**

## Open questions resolved during Phase 0

The list of open questions in the original handoff is largely closed:

- ~~rmcp SDK version specifics.~~ Pinned `rmcp = "1.7"` (was `0.1`). Spike 0.2 found the proc-macro pattern; the layout adaptation lives in `docs/specs/...md` §13.1 and the `src/server.rs` single-impl-block.
- ~~reis crate libei version compatibility.~~ EIS handoff verified end-to-end against KWin 6 via `examples/eis-keystroke-prober/`. `flags=1` is the sender bit (undocumented; recorded in the spikes postscript).
- ~~atspi crate's async surface.~~ Pin updated to `0.30.0`; the tokio-feature pattern is unchanged. Phase 1 will exercise it concretely.
- **Still open — KWin Scripting D-Bus output capture.** During Task 0 spikes we used the `print()` → journald path for the prober's window enumeration. Phase 1's `window_query` should pick: kdotool-style write-to-temp-file vs. our journald capture vs. the in-script `callDBus` round-trip pattern. The choice is a Phase 1 plan-level concern.
- **Still open — KGlobalAccel cancel-shortcut grab.** Phase 3 will deal with this; it's not on Phase 1's critical path.
- **Still open — `org.freedesktop.Notifications` action-button delivery.** Phase 3 (policy gate / per-app approval) needs this. Phase 1 only uses notifications for acquire/release events, which Phase 0 already verified.

### Phase 1's pre-plan checklist

Before invoking `superpowers:writing-plans` for Phase 1, decide:

1. KWin Scripting output capture mechanism (the three options above).
2. Whether `window_query` returns a flat list or a per-screen grouping by default (spec §6.1 leaves this open).
3. Whether `screenshot` masks terminals in Phase 1 (spec §10 layer 7 — but layer 7 is wired through the policy gate which is Phase 3; for Phase 1 the masking could be a static class-list filter, then policy-gate-integrated in Phase 3).

## Things the user explicitly chose

- **C-mode session switching** (live + virtual within one conversation) was initially proposed and **abandoned** after deeper research showed it was untested and added internal state-confusion bugs. Final decision: mode-per-process, two MCP server entries to configure if you want both. See ADR-0003.
- **Rust, accepting the LOC cost** versus Python's faster iteration. The user explicitly asked "can we do the same in Rust?" and accepted when the answer was yes.
- **8–9 extra days of MVP scope** for the Anthropic safety features (ADR-0007), explicitly because the user wanted quality close to or above Anthropic's product.
- **No GitHub remote yet.** This is a personal-use project. Do not push to any remote unless atassis says so.

## Things deferred to future work

From spec §15:

- Journey tools (`save_current_document()`, `open_app_and_wait_until_ready()`)
- CLI+Skills delivery alongside MCP
- Streamable-HTTP transport for remote driving
- Multi-compositor support (GNOME / wlroots)
- Persistent per-app approval across sessions
- App-category tier auto-discovery
- `.mcpb` extension bundle packaging (closest substitute for "bundled-with-host")

None of these are blockers. They are intentional out-of-scope items.

## Reading order if you have 30 minutes

1. `README.md` — 2 minutes, project pitch
2. `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` §1, §2, §3, §4 — 10 minutes, the design philosophy
3. `docs/specs/...md` §6 (tool catalogue) and §6.0 (policy gate) — 10 minutes, the tool surface
4. `docs/adr/0007-adopt-anthropic-safety-architecture.md` — 5 minutes, the most recent decision
5. `docs/comparison-with-anthropic-computer-use.md` — 5 minutes, why this exists vs. what's already shipping

If you have 5 minutes: just the README and §1-§4 of the spec.

## How to resume work

In a fresh Claude Code session at the repo root:

```
# 1. Re-orient.
cat CLAUDE.md
cat docs/HANDOFF.md
git log --oneline

# 2. If atassis has reviewed the spec and approves:
#    Invoke /superpowers:writing-plans (or ask Claude to)
#    Plan should target Phase 0 first.

# 3. After the plan is written and reviewed:
#    Invoke /superpowers:executing-plans (or subagent-driven-development)
#    First task: cargo init + skeleton.
```

## How NOT to resume work

- Do not start writing Rust without a plan from `writing-plans`.
- Do not skip the spec review.
- Do not modify accepted ADRs in place; write a new ADR if the decision changes.
- Do not push to a remote without explicit ask.
- Do not run `sudo` directly; provide commands for the user to run.

---

That's everything you need to pick up cleanly. The conversation that produced these docs is captured in the docs themselves; nothing material lives only in transcripts.
