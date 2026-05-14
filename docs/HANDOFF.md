# Handoff — pick up where we left off

A snapshot of project state. Read this first if you are returning to the project (Claude session or otherwise) after time away.

---

## TL;DR — what to do first in a fresh session

```sh
cat CLAUDE.md             # project conventions (incl. global ones at ~/.claude/CLAUDE.md)
cat docs/HANDOFF.md       # this file
git log --oneline | head -20
git tag -l                # phase-0 should be present
cargo test --workspace    # 37 tests; should all pass in <2 s after build
```

Then **verify Phase 0 still works end-to-end** with the smoke procedure in `tests/integration/SMOKE.md` — pipe a hand-built JSON-RPC fixture to the release binary and confirm 3 responses come back with the expected shape. Takes 30 seconds. If that's green, the project is exactly where this handoff says it is.

After verification, the concrete next action is to **plan Phase 1**:

1. Read the "Phase 1 pre-plan recommendations" below; the three decisions are pre-answered.
2. Invoke `superpowers:writing-plans` targeting Phase 1 (read-only tools per spec §14 row 1).
3. Then `superpowers:executing-plans` (or `subagent-driven-development`) to implement.

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
| Visual prompt injection is host-side, not server-side | 0008 | VPI-Bench shows CUAs vulnerable to pixel-content attacks; trust-string discriminator only covers strings — pixel quarantine is the model host's job |

The MVP is sized at **18–23 working days for phases 0–5**, plus open-ended polish. Realistic 5–7 calendar weeks at part-time pace. Phase 0 took **~1 working day** including the EIS keystroke POC bonus.

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

### Phase 1 pre-plan recommendations

Three decisions need answers before `writing-plans` runs for Phase 1. My recommendations (you should review and override if you disagree before invoking the skill):

1. **KWin Scripting output capture: write-to-temp-file.**

   Three viable mechanisms surfaced during Phase 0:
   - kdotool-style write-to-temp-file: KWin script calls `callDBus("org.freedesktop.DBus.Properties", ...)` or just opens a file. Caller reads after script exits.
   - journald `print()` capture: KWin script `print()`s, caller scrapes `journalctl --user -t kwin_wayland --since "Ns ago"`. **What we used during Phase 0 spikes** — works but the journald scrape is racy and rate-limited.
   - In-script `callDBus` round-trip: script calls back into a D-Bus method on the caller's side. Requires the caller to host a D-Bus service — heavyweight.

   **Recommend write-to-temp-file** (option 1). Reasons: deterministic (no journald rate limiting), zero coupling to systemd, easy to test, matches kdotool's battle-tested pattern. Path: `$XDG_RUNTIME_DIR/kde-mcp/script-<nonce>.out` with cleanup-on-read. Each script invocation gets a fresh nonce so concurrent calls don't collide.

2. **`window_query` default grouping: flat list, with `screen` field on each entry.**

   Spec §6.1 leaves `format: "ids" | "titles" | "full"` open. **Recommend flat list always**, with each entry carrying `screen: <output-name>` so a per-screen grouping is trivially computable agent-side. The `format` parameter then controls *which fields* are returned, not *how the list is structured*. Reasons: simpler schema (one shape), agent can group if it cares, fewer pass-through `match` arms in the agent's prompt.

3. **Terminal-masking in Phase 1: static class-list filter, wired through the policy gate in Phase 3.**

   Spec §10 layer 7 (terminal-excluded screenshots) is wired through the policy gate (§6.0 step 6 / Phase 3). But Phase 1 ships `screenshot` standalone — without the gate. **Recommend Phase 1 implements terminal masking with a hardcoded class list** (the spec's `konsole | yakuake | alacritty | kitty | wezterm | foot | ghostty | tilix | gnome-terminal | xterm | xfce4-terminal | ptyxis | warp-terminal`), then Phase 3 replaces the hardcoded list with the policy-table lookup. Phase 1's `screenshot` shouldn't ship without *some* terminal masking — defense in depth means we mask even before the policy gate exists.

If you accept all three recommendations, the Phase 1 plan can be written without further user input.

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

## Where things live

```
kde-mcp/
├── Cargo.toml                          # Phase 0 deps only; expand per phase
├── rust-toolchain.toml                 # stable, edition 2024, MSRV 1.85
├── src/
│   ├── main.rs                         # CLI → lock → notify → bootstrap → rmcp serve
│   ├── lib.rs                          # module declarations
│   ├── cli.rs                          # --mode + --vnc parsing
│   ├── error.rs                        # ToolError enum (13 codes + NotImplemented)
│   ├── audit.rs                        # JSONL writer + redaction
│   ├── trust.rs                        # Trust enum + Tagged<T> wrapper
│   ├── refs.rs                         # opaque-ref TTL store
│   ├── lock.rs                         # per-mode lockfile + in-process singleton
│   ├── notify.rs                       # acquire/release KDE notifications
│   ├── server.rs                       # KdeMcp struct, single #[tool_router] impl, 16 tools
│   ├── mode/                           # mode/{mod,live,virtual}.rs
│   └── tools/                          # 12 files, 15 logical tools — all stubs
├── tests/
│   ├── integration.rs                  # wrapper for the cargo "integration" binary
│   └── integration/                    # cli_parse, lockfile, tools_list + SMOKE.md
├── examples/eis-keystroke-prober/      # Standalone POC for Phase 2 — see README
├── docs/
│   ├── HANDOFF.md                      # this file
│   ├── specs/2026-05-13-kde-...md      # the spec
│   ├── plans/2026-05-14-phase-0-...md  # the Phase 0 plan we executed
│   ├── adr/{0001..0008}                # accepted decisions
│   ├── research/                       # snapshot research + Phase 0 spikes
│   └── comparison-with-anthropic-computer-use.md
└── packaging/                          # .desktop file, policy.toml example
```

## Reading order if you have 30 minutes

1. **This file's TL;DR section** — 1 minute, fastest re-orientation.
2. `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` §1, §2, §3, §4 — 10 minutes, design philosophy.
3. `docs/specs/...md` §6 (tool catalogue) + §6.0 (policy gate) + **§13.1 (library pins / module-layout adaptation)** — 10 minutes.
4. `docs/research/2026-05-14-phase-0-spikes.md` — 5 minutes, the postscript at the end has live-verified findings worth knowing before Phase 2.
5. `docs/adr/0008-visual-prompt-injection-host-side-defence.md` — 3 minutes, most recent decision.

If you have 5 minutes: just this file's TL;DR + spec §1–§4.

## How NOT to resume work

- Do not modify accepted ADRs in place. Write a new ADR if the decision changes; supersede the old one.
- Do not start Phase 1 code without first invoking `superpowers:writing-plans` for Phase 1.
- Do not push to a remote without atassis's explicit ask. There is no remote configured.
- Do not run `sudo` directly; provide commands for the user to run if root is needed.
- Do not skip the smoke test in `tests/integration/SMOKE.md` after any change to `src/{main,server}.rs` or `Cargo.toml`.
- Do not delete `examples/eis-keystroke-prober/`. It's the canonical reference for Phase 2's keystroke path.
- Do not pin `schemars = "0.8"` — must be `"1.0"` to match rmcp 1.7's internal version (spike 0.2 sub-finding).

## Quick recipes

**Re-run the JSON-RPC smoke against the release binary:**

```sh
cargo build --release
# Copy the inline script in tests/integration/SMOKE.md §A.1 to /tmp/kde-mcp-smoke.sh
# (or just inline the heredoc).
chmod +x /tmp/kde-mcp-smoke.sh
/tmp/kde-mcp-smoke.sh "$PWD/target/release/kde-mcp" > /tmp/out.jsonl
python3 -c "import json; ls=open('/tmp/out.jsonl').read().splitlines(); print(len(ls), 'responses'); [print(json.loads(l)['id'] if 'id' in json.loads(l) else 'notif', '->', list(json.loads(l).get('result', {}).keys())[:3]) for l in ls]"
```

**Re-run the EIS keystroke POC** (with the target window focused):

```sh
cd examples/eis-keystroke-prober
cargo run --release
```

**See exactly what changed since the design phase:**

```sh
git log --oneline phase-0
```

---

That's everything you need to pick up cleanly. The conversation that produced these docs is captured in the docs themselves; nothing material lives only in transcripts.
