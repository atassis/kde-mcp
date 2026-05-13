# Handoff — pick up where we left off

A snapshot of project state at the moment the design conversation paused. Read this first if you are returning to the project (Claude session or otherwise) after time away.

---

## Current status (2026-05-13)

- **Phase:** design-only. No source code yet.
- **Last commit:** `5d2a943 feat(design): adopt Anthropic computer-use safety architecture (ADR-0007)`
- **Branch:** `main` (single branch; no remote configured)
- **Working tree:** clean

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

1. **Spec review by atassis.** (Brainstorming skill step 7.) The user wanted to defer this to a fresh session. The spec is committed and complete; just needs to be read.
2. **Invocation of the `superpowers:writing-plans` skill** to turn the spec into an implementation plan with TDD-shaped tasks. (Brainstorming skill step 8.) This is the **terminal state of the brainstorming flow.**
3. **Phase 0 implementation:** `cargo new`, rmcp wiring, `--mode` flag, audit log, error taxonomy, single-session lock + KDE notifications, stub all 15 tools to return `not_implemented`. See spec §14.
4. **Phases 1–5** per the table in spec §14.
5. **Phase 6 polish.**

## Open questions to resolve before implementation

The spec is honest about what it doesn't commit to:

- **rmcp SDK version specifics.** `Cargo.toml` declares `rmcp = "0.1"` but the API may have moved. Verify before Phase 0 with `cargo search rmcp` and `cargo doc --open` — adjust the `mod.rs` patterns to match the current SDK.
- **reis crate libei version compatibility.** Cargo declares `reis = "0.4"`. Test the EIS-fd handoff dance against the user's installed `libei` version (`pkg-config --modversion libei`).
- **atspi crate's async surface.** `atspi = "0.22"` with `tokio` feature. Validate against the documented event loop before structuring `src/atspi/connection.rs`.
- **KWin Scripting D-Bus calling pattern.** The spec says "load script, run, read output via signal or temp file, unload" (kdotool's pattern). Decide concretely: signal-based output capture vs. write-to-temp-file. Probably write-to-temp-file is simpler for MVP.
- **KGlobalAccel registration semantics.** The cancel-shortcut design assumes KGlobalAccel grants compositor-level key grab. Verify against KDE Plasma 6.5+ behaviour; some shortcut classes require additional setup.
- **`org.freedesktop.Notifications` action button event delivery.** The per-app approval design relies on receiving the user's button click via D-Bus signal. Confirm KDE's notification daemon (or `plasma-workspace`'s notification server) routes button clicks back consistently — older specs let action data be lost on dismiss.

These are all things the writing-plans phase should turn into explicit "verify before phase X" tasks.

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
