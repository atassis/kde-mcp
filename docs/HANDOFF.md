# Handoff — pick up where we left off

A snapshot of project state. Read this first if you are returning to the project (Claude session or otherwise) after time away.

---

## TL;DR — what to do first in a fresh session

```sh
cat CLAUDE.md             # project conventions (incl. global ones at ~/.claude/CLAUDE.md)
cat docs/HANDOFF.md       # this file
git log --oneline | head -20
git tag -l                # phase-0 should be present (phase-1 not yet tagged)
cargo test --workspace    # 57 tests; should all pass in <5 s after build
```

Phase 1 is **in flight, ~70% complete.** Tasks 0–10 of the 14-task Phase 1 plan are merged; Tasks 11–14 (AT-SPI infrastructure, `inspect`, `observe`, final verification + tag) are pending and require user direction. See "Phase 1 progress" below.

**Immediate next action when atassis returns:**

1. Read `docs/plans/2026-05-14-phase-1-readonly-tools.md` §"Pre-plan caveats and resolved decisions" for the **one design relabel** that happened during planning. HANDOFF said "write-to-temp-file"; reality is the kdotool-style caller-hosted D-Bus receiver (KWin's QJSEngine has no file I/O). Implementation is ~60 LoC at `src/kwin/scripting.rs`. Confirm or override.
2. Install `wl-clipboard` so `clipboard_read`'s live spot-check can run: `sudo pacman -S wl-clipboard` (Arch).
3. Run the live smoke checks deferred during autonomous execution:
   - `text_editor.view` — already unit-tested over tempfiles; trivial.
   - `clipboard_read` — `wl-paste` against real session after install.
   - `window_query` — `--mode=live` invocation.
   - `screenshot` — `--mode=live` invocation.
   Each `--mode=live` invocation fires the "Claude is using your computer" KDE notification, which is why they were deferred.
4. Decide on Task 11 (AT-SPI infrastructure) scope: the plan's first cut returns `focused_element = null` with a warning rather than running a background focus-tracker. Confirm or override.
5. Resume the plan from Task 11 in a fresh subagent / executing-plans session.

---

## Current status (2026-05-14, evening)

- **Phase:** **Phase 1 in flight** — 10 of 14 tasks implemented + merged; 4 remaining.
- **Last commit:** `864a862 feat(phase-1): screenshot tool with terminal-class masking`.
- **Tag:** `phase-0` (local; not pushed). `phase-1` not yet tagged (waiting on Tasks 11–14).
- **Branch:** `main` (single branch; no remote configured).
- **Working tree:** clean.
- **Tests:** 57 passing (35 unit + 22 integration). `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Phase 1 progress

| Task | Status | Notes |
|---|---|---|
| 0 — Spikes | Done | `docs/research/2026-05-14-phase-1-spikes.md` — confirmed atspi 0.30 surface, KWin Plasma 6 globals (one rename: `activeOutput` → `activeScreen`), ScreenShot2 sigs, wl-paste install gap. |
| 1 — Cargo + skeleton | Done | New deps: atspi, image, zvariant, uuid, futures-util, sha2, base64, regex-lite, nix, tempfile. New module trees: `src/atspi/`, `src/kwin/`, `src/util/`. |
| 2 — Audit dispatch | Done | Every tool dispatch now writes one `AuditRecord` per call. Verified via smoke. All 15 tool free-functions take `&KdeMcp` as first arg. |
| 3 — text_editor.view | Done | Pure file I/O; 6 integration tests. |
| 4 — clipboard_read | Done | Spawns `wl-paste`; 1 offline test passes via the "wl-paste not installed" branch. Live verification gated on `wl-clipboard` install. |
| 5 — kwin script runner | Done | Caller-hosted D-Bus receiver (`org.kde.kdemcp.ScriptSink1.S<uuid>`) per-invocation. 2 offline tests on the JS template renderer. **Untested live** — no smoke run from here on. |
| 6 — window_query | Done | Compose JS, parse JSON, project to format. 3 offline unit tests. **Untested live.** |
| 7 — ScreenShot2 proxy | Done | zbus proxy for 6 capture methods. Compile-only verification. |
| 8 — screenshot tool | Done | Region encoding (untagged enum: string OR `[x,y,w,h]`), capture, PNG re-encode. **Untested live.** |
| 9 — image utils | Done | Resize-to-long-edge + black-rectangle mask. 6 unit tests including bounds-clamping. |
| 10 — terminal masking | Done | Folded into commit `864a862`. Self-shot exemption implemented (capturing `window:<terminal-ref>` masks other terminals but not the targeted one). Fallback to unmasked + warning if window-enumeration fails. **Untested live.** |
| 11 — AT-SPI infra | **Pending** | Plan ships pseudocode for `walk()`; final fn names per Task 0.1 spike. Risky — needs live a11y bus to validate. See plan §Task 11. |
| 12 — inspect | **Pending** | Depends on Task 11. Plan defers `inspect target="window:<ref>"` to Phase 4 (window→AT-SPI mapping). |
| 13 — observe | **Pending** | Depends on Tasks 5 + 11. Plan ships `focused_element = null + warning` for the first cut. |
| 14 — verify + tag | **Pending** | Full SMOKE.md refresh + `git tag phase-1`. |

### What's been verified

- 57 cargo tests pass (unit + integration).
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
- One end-of-Task-2 smoke run confirmed audit-log dispatch writes per-call (`{"tool":"observe","ok":false,"duration_ms":0,"code":"not_implemented"}` lands in `~/.local/state/kde-mcp/audit.jsonl`).

### What was deliberately NOT live-tested (per user's "don't change session state" directive)

- Tasks 5–10 wire-level invocations against the real KWin session. Each `--mode=live` startup fires the "Claude is using your computer" KDE notification, and a `screenshot` call would capture the user's actual screen. Recommended live-test order after user returns:
  1. `window_query action=list` (the lightest live test; KWin script + parse).
  2. `screenshot region=active_screen` (verifies ScreenShot2 + image encode).
  3. `screenshot region=full` (verifies terminal masking against a real desktop).
  4. `screenshot region=window:<ref>` after grabbing a `window_query` ref.
- `clipboard_read` live verification — gated on `pacman -S wl-clipboard`.

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
2. ~~Phase 0 plan + implementation.~~ Done 2026-05-14.
3. ~~Phase 1 plan.~~ Done 2026-05-14, at `docs/plans/2026-05-14-phase-1-readonly-tools.md`.
4. ~~Phase 1 Tasks 0–10.~~ Done 2026-05-14 (autonomous).
5. **Phase 1 Tasks 11–14:** AT-SPI infrastructure, `inspect`, `observe`, live smoke + `phase-1` tag. **Next concrete step:** see TL;DR above; resume from Task 11.
6. **Phases 2–5** per spec §14.
7. **Phase 6 polish.**

## Open questions resolved during Phase 0 + Phase 1

- ~~rmcp SDK version specifics.~~ Pinned `rmcp = "1.7"` (was `0.1`). Spike 0.2 found the proc-macro pattern; the layout adaptation lives in `docs/specs/...md` §13.1 and the `src/server.rs` single-impl-block.
- ~~reis crate libei version compatibility.~~ EIS handoff verified end-to-end against KWin 6 via `examples/eis-keystroke-prober/`. `flags=1` is the sender bit (undocumented; recorded in the Phase 0 spikes postscript).
- ~~atspi crate's async surface.~~ Pin confirmed at `0.30.0`. `AccessibilityConnection::new()`; no direct "focused element" property — focus is tracked via `FocusEvents` signals. Phase 1 first-cut returns null + warning rather than running a background focus-tracker.
- ~~KWin Scripting D-Bus output capture.~~ Resolved via Phase 1 spike: KWin's QJSEngine has zero file I/O globals. The only path is `callDBus`. Implementation uses the **kdotool-style caller-hosted D-Bus receiver** pattern (~60 LoC at `src/kwin/scripting.rs`). The HANDOFF's earlier "write-to-temp-file" label was misleading — see `docs/plans/2026-05-14-phase-1-readonly-tools.md` §"Pre-plan caveats" for the full reasoning.
- ~~KWin Plasma 6 scripting global names.~~ Verified via `KDE/kwin/master/src/scripting/workspace_wrapper.h`: `workspace.activeWindow` (was `activeClient` in Plasma 5), `workspace.windowList()` (method, not property), `workspace.activeScreen` (NOT `activeOutput` as the plan initially drafted). Spike findings applied during Task 6/13 implementations.
- ~~ScreenShot2 D-Bus signatures.~~ Live-introspected on dev machine; verified all 6 `Capture*` method shapes match the Phase 1 plan's draft. Interface `Version` reads `5`.
- **Still open — KGlobalAccel cancel-shortcut grab.** Phase 3 will deal with this; not on Phase 1's critical path.
- **Still open — `org.freedesktop.Notifications` action-button delivery.** Phase 3 (policy gate / per-app approval) needs this. Phase 1 only uses notifications for acquire/release events, which Phase 0 already verified.
- **Still open — `wl-clipboard` not installed on dev machine.** Install via `pacman -S wl-clipboard` for live `clipboard_read` verification. Offline test passes via the "wl-paste missing" error branch.

### Phase 1 pre-plan decisions (all approved by user + applied)

The three pre-plan recommendations were approved with a single relabel:

1. **KWin Scripting output capture.** Labelled "write-to-temp-file" in HANDOFF; reality is **kdotool-style caller-hosted D-Bus receiver** (KWin's QJSEngine has no file I/O — only `callDBus`). Implemented at `src/kwin/scripting.rs`. The HANDOFF wording was based on a misreading of kdotool's mechanism; the actual approach is what kdotool uses today. See `docs/plans/2026-05-14-phase-1-readonly-tools.md` §"Pre-plan caveats" for the full reasoning.

2. **`window_query` default grouping.** Flat list with `screen` field per entry; `format: "ids" | "titles" | "full"` controls fields, not list structure. Applied verbatim in `src/tools/window.rs`.

3. **Terminal-masking.** Hardcoded class list in `src/util/image.rs::TERMINAL_CLASSES`; Phase 3 replaces with policy-table lookup. Self-shot exemption: capturing `window:<terminal-ref>` masks other terminals but not the targeted one. Applied in `src/tools/screenshot.rs::list_terminal_rects`.

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
├── Cargo.toml                          # deps grow per phase; Phase 1 adds atspi, image, etc.
├── rust-toolchain.toml                 # stable, edition 2024, MSRV 1.85
├── src/
│   ├── main.rs                         # CLI → lock → notify → bootstrap → rmcp serve
│   ├── lib.rs                          # module declarations
│   ├── cli.rs                          # --mode + --vnc parsing
│   ├── error.rs                        # ToolError enum (15 codes + NotImplemented)
│   ├── audit.rs                        # JSONL writer + redaction
│   ├── trust.rs                        # Trust enum + Tagged<T> wrapper
│   ├── refs.rs                         # opaque-ref TTL store
│   ├── lock.rs                         # per-mode lockfile + in-process singleton
│   ├── notify.rs                       # acquire/release KDE notifications
│   ├── server.rs                       # KdeMcp struct, dispatch_audited, 16 #[tool] shims
│   ├── mode/                           # mode/{mod,live,virtual}.rs
│   ├── tools/                          # 12 files, 15 logical tools
│   ├── atspi/                          # connection / tree / ref_encoding — skeletons (Phase 1 Task 11 fills in)
│   ├── kwin/                           # scripting.rs (caller-hosted DBus sink), screenshot.rs (ScreenShot2 proxy)
│   └── util/                           # image.rs (resize + terminal mask)
├── tests/
│   ├── integration.rs                  # wrapper for the cargo "integration" binary
│   └── integration/                    # cli_parse, lockfile, tools_list, text_editor, clipboard_offline + SMOKE.md
├── examples/eis-keystroke-prober/      # Standalone POC for Phase 2 — see README
├── docs/
│   ├── HANDOFF.md                      # this file
│   ├── specs/2026-05-13-...md          # the spec
│   ├── plans/2026-05-14-phase-0-...md  # Phase 0 plan (executed)
│   ├── plans/2026-05-14-phase-1-...md  # Phase 1 plan (10/14 executed)
│   ├── adr/{0001..0008}                # accepted decisions
│   ├── research/                       # snapshot research + Phase 0 + Phase 1 spikes
│   └── comparison-with-anthropic-computer-use.md
└── packaging/                          # .desktop file, policy.toml example
```

## Reading order if you have 30 minutes

1. **This file's TL;DR section** — 1 minute, fastest re-orientation.
2. `docs/plans/2026-05-14-phase-1-readonly-tools.md` §"Pre-plan caveats" + Task 11 — 5 minutes, the bits that need user input next.
3. `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` §1, §2, §3, §4 — 10 minutes, design philosophy.
4. `docs/specs/...md` §6 (tool catalogue) + §6.0 (policy gate) + **§13.1 (library pins / module-layout adaptation)** — 10 minutes.
5. `docs/research/2026-05-14-phase-1-spikes.md` — 4 minutes, the Phase 1 findings (atspi 0.30, KWin scripting renames, ScreenShot2 sigs).

If you have 5 minutes: just this file's TL;DR + the §"Pre-plan caveats" in the Phase 1 plan.

## How NOT to resume work

- Do not modify accepted ADRs in place. Write a new ADR if the decision changes; supersede the old one.
- Do not start Phase 1 Task 11 code without first re-reading the plan's Task 11 step — the AT-SPI API surface is the last load-bearing unknown.
- Do not push to a remote without atassis's explicit ask. There is no remote configured.
- Do not run `sudo` directly; provide commands for the user to run if root is needed.
- Do not skip the smoke test in `tests/integration/SMOKE.md` after any change to `src/{main,server}.rs` or `Cargo.toml`.
- Do not delete `examples/eis-keystroke-prober/`. It's the canonical reference for Phase 2's keystroke path.
- Do not pin `schemars = "0.8"` — must be `"1.0"` to match rmcp 1.7's internal version (spike 0.2 sub-finding).
- Do not run `target/release/kde-mcp --mode=live` casually — it fires a KDE notification each startup. Batch live smoke runs when the user is ready.

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
