# Verification runbook — design

**Status:** draft (awaiting user review)
**Author:** AI session 2026-05-15
**Supersedes:** none

---

## Purpose

A self-contained doc + log layout that lets a fresh Claude Code session test `kde-mcp` end-to-end as a product, autonomously: read steps, call MCP tools, validate responses against acceptance criteria, capture structured artefacts, apply one known fix per failure, and write a one-page summary.

Two things this *is not*:

- **Not a Rust test framework.** No code changes. Pure markdown + a fixed log directory layout. The session is the executor.
- **Not an AI-debug loop.** The "auto-fix" is a static `KNOWN-FIXES.md` lookup table — signature → mechanical command. Anything outside the table is logged and skipped, never debugged on the fly.

The runbook complements `docs/TESTING-CHECKLIST.md` (which lists *what* to verify) by giving the *how*: stepwise, parseable instructions.

## Goals

1. A fresh Claude Code session can run the full verification by being told only "follow `docs/runbook/AUTONOMOUS-VERIFICATION.md`."
2. Every step produces a machine-parseable artefact (JSON response payload, audit-log excerpt, screenshot, OCR output) under `docs/runbook/runs/<id>/`.
3. Failures don't halt the run; they log and the session continues unless an explicit dependency requires the failed step.
4. Resumability — if a run dies, the next session can read `runs/<latest>/progress.jsonl`, find the last completed step, and pick up there.
5. The runbook stays in lock-step with `docs/TESTING-CHECKLIST.md`: every item in the checklist maps to one or more runbook steps via a `tc-ref` field.

## Non-goals

- Multi-machine portability of `runs/` directories. The artefacts are tied to the host KDE session.
- Detecting flakiness via repeated runs. One run, best-effort, log what happened.
- A web dashboard, CI integration, or any out-of-band reporting. Logs are markdown + JSONL only.
- Fixing novel bugs autonomously. The known-fixes table is hand-curated; novel issues become entries in `docs/TESTING-CHECKLIST.md`'s bug-bounty section for human follow-up.

## Architecture

### Layout

```
docs/runbook/
├── AUTONOMOUS-VERIFICATION.md     # entry point — the agent reads this first
├── KNOWN-FIXES.md                 # failure-signature → mechanical-fix table
├── scenarios/
│   ├── S1-situational-awareness.md
│   ├── S2-drive-kcalc.md
│   ├── S3-ocr-fallback.md
│   ├── S4-clipboard-transport.md
│   ├── S5-window-management.md
│   └── S6-brave-form-fill.md
└── runs/                          # gitignored; agent-managed
    └── 2026-05-15-1530/
        ├── progress.jsonl         # one line per step: {id, status, started_at, ended_at, notes}
        ├── summary.md             # written at run end; pass/fail counts + headline regressions
        ├── env.json               # captured environment: kde version, kwin version, atspi version, …
        ├── L1.1-tools_list.json
        ├── L2.3-screenshot.png
        ├── L2.4-clipboard.json
        ├── …
        └── audit-excerpts/
            ├── L2.4.jsonl
            └── …
```

`docs/runbook/runs/` is added to `.gitignore` to keep response payloads (potentially containing screenshot pixels, clipboard contents, file paths) out of the repo.

### Layers

The runbook contains four layers, executed in order:

#### Layer 1 — Health & environment (≈6 steps, <1 min, no live side-effects)

Verifies the *running* kde-mcp MCP server is healthy without spawning a second instance (which would fail the live lockfile). Hermetic wire-level smoke — `initialize`, `tools/list` shape, lockfile presence — is already covered by `tests/integration/` and is **not duplicated here**. This layer instead validates the deployed state:

- `claude mcp list` shows `kde-mcp: ... - ✓ Connected`.
- Run `cargo test --workspace` once — must pass (110/110). Catches regressions in deployed code via the hermetic suite.
- Inspect `~/.local/bin/kde-mcp` exists and is executable.
- Inspect `~/.local/share/applications/kde-mcp.desktop` exists with `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`.
- `pgrep -a kde-mcp` returns exactly one process when a Claude Code session is open, none when it's not.
- Capture `env.json`: KDE Plasma version (`plasmashell --version`), KWin version (`kwin_wayland --version`), atspi version (best-effort via `qdbus org.a11y.Bus`), monitor list from `xrandr --listmonitors`.

If the installed binary is missing, the known-fix is `./packaging/install.sh`.

#### Layer 2 — Per-tool acceptance (≈16 steps, ~5 min, mostly read-only)

Each of the 16 tools called once via the live MCP session (Claude Code's already-registered `kde-mcp`). Read-only tools run first (no approval prompts since they're allowlisted in `~/.claude/settings.json`); destructive tools batched at the end so all approval prompts arrive together.

Per tool:
- Probe args chosen to be minimal and obvious: `observe(detail=summary)`, `text_editor.view /etc/hostname`, `screenshot(region=active_screen)`, `bash(command="echo hello", restart=false)`, etc.
- Acceptance criteria validated structurally (response has expected keys with non-empty values) — no exact-match against captured payloads (those drift with KDE state).
- Response payload written verbatim to `runs/<id>/L2.<n>-<tool>.json`.

#### Layer 3 — Real-product scenarios (≈6 scenarios, ~10–15 min)

Six end-to-end sequences mimicking what someone would actually ask Claude to do:

**S1 Situational awareness.** `observe(detail=interactive)`. Acceptance: ≥1 window, `focused_window` non-null, `screens` matches `xrandr --listmonitors` count.

**S2 Drive KCalc.**
1. `bash launch=kcalc` (or `kcalc &` if launch flag unsupported).
2. `wait condition=window_appears match={class: "org.kde.kcalc"}` timeout 5s.
3. `window_control action=focus window_uuid=<from wait>`.
4. `keyboard.type` "47*23".
5. `keyboard.key "Return"`.
6. `inspect target=window:<uuid>` filter role="text" → grab display value.
7. Acceptance: display reads "1,081" or "1081".
8. Cleanup: `window_control action=close window_uuid=<uuid>`.

**S3 OCR fallback.** First-run only: generate `~/.cache/kde-mcp-runbook/ocr-fixture.png` via `magick convert -background white -fill black -size 400x100 caption:"kde-mcp ocr probe ${run_id}" "$out"`. Then:
1. `bash` to open `gwenview <fixture>`.
2. `wait` for the window.
3. `read_text region=window:<uuid> lang=eng`.
4. Acceptance: returned text contains "kde-mcp ocr probe" substring.
5. Cleanup: close gwenview.

**S4 Clipboard transport.**
1. Save current clipboard: `clipboard_read`.
2. `clipboard_write content="kde-mcp-probe-${run_id}"`.
3. `clipboard_read`.
4. Acceptance: matches probe string.
5. Restore: `clipboard_write content=<saved>`.

**S5 Window management.**
1. `window_query action=list` → pick the focused window's UUID.
2. Save its geometry.
3. `window_control action=move geometry=[100,100,800,600]`.
4. `observe` → verify geometry changed.
5. Acceptance: new geometry matches request.
6. Restore: `window_control action=move geometry=<saved>`.

**S6 Brave form-fill.**
1. `window_query` → find Brave window (class contains `brave-browser`).
2. `window_control action=focus window_uuid=<uuid>`.
3. `keyboard.key "ctrl+l"` (focus address bar).
4. `keyboard.type "kde-mcp-marker-${run_id}"`.
5. `inspect target=window:<uuid>` filter role contains "entry" / "textbox" → find URL bar.
6. Acceptance: the AT-SPI value of the address-bar element contains the marker.
7. Cleanup: `keyboard.key "Escape"`. Don't press Return — we don't actually navigate.

Brave is the user's daily browser; if it's not running when the runbook starts, S6 is logged as `skipped(precondition: Brave not running)` and other scenarios continue.

#### Layer 4 — Bug-bounty probes (≈4 steps, ~3 min)

A subset of the six items at `docs/TESTING-CHECKLIST.md:172`. Only the ones observable from *outside* the binary make sense in this layer; ones that need EIS-frame instrumentation are explicit non-goals for v1 (would require build flag + log capture; tracked as a future addition).

- **B3 (KEEP):** `clipboard_write content=<base64-encoded random 32 bytes> encoding=base64`; `clipboard_read`; decode response; assert bytes round-trip.
- **B4 (CONDITIONAL on multi-monitor):** Detect `>1` monitor in `env.json`. If yes: `screenshot region=[x,y,w,h]` straddling the seam; record returned dims and visually-inspect that both monitor regions are populated (save PNG for human eyes). If single-monitor: skip with `skipped(precondition: single monitor)`.
- **B5 (KEEP):** Open Dolphin via `bash launch=dolphin`; `wait` for window; `inspect target=window:<uuid> max_elements=10000`; assert returns in <5 s with `truncated=false` *or* `truncated=true` (either is OK — the failure is hanging / OOM); record element_count.
- **B6 (KEEP):** `wait condition=screen_change pixel_threshold=0.05 timeout_ms=500` against a confirmed-static screen; assert `matched=false` and `code="timeout"` within 600 ms.

**Deferred to v2:**
- **B1** (shift-release between caps): needs EIS-frame log; outside the binary's externally-observable surface.
- **B2** (mouse double-click coalescing): needs a target app's event log or a custom probe widget; out of scope.

Each probe writes its captured evidence under `runs/<id>/B<n>-*.{json,log,png}`. Acceptance criteria are pass/concerning/fail; "concerning" cases get logged but don't count as failures.

### Per-step format

Each step in every layer follows the same parseable template:

```markdown
### L<layer>.<n> — <short title>
**TC-Ref:** TESTING-CHECKLIST.md §<section>
**Goal:** <one sentence>
**Pre:** <what must be true before; "none" if standalone>
**Depends-on:** <other step ids, comma-separated; empty = independent>
**Steps:**
1. ...
2. ...
**Accept:** <one or more structural assertions on the response>
**Artefacts:** <files the agent must write under runs/<id>/>
**On fail:**
  - <signature> → <known fix from KNOWN-FIXES.md, or "log and skip">
  - else: log full context, mark step `failed`, continue.
**Cleanup:** <state to restore; "none" if idempotent>
```

The agent is told to read this template upfront and apply it uniformly. No prose freedom — every step has the same shape.

### Known-fixes table

`docs/runbook/KNOWN-FIXES.md` is a flat markdown table with three columns: signature regex, fix command, retry-once flag. Examples:

| Signature (regex over error code + detail) | Fix | Retry |
|---|---|---|
| `not_authorized.*ScreenShot2` | `kbuildsycoca6 --noincremental` | yes |
| `binary not found at \\$BIN` | `./packaging/install.sh --skip-mcp` | yes |
| `lock_unavailable` | (none — abort run; another session holds the live lock) | no |
| `wl-paste: command not found` | (none — skip remaining clipboard steps) | no |
| `tesseract.*not found` | (none — skip S3) | no |
| `external_command_failed.*magick` | (none — skip S3 fixture generation) | no |
| `kde-mcp .* connection refused` | restart Claude Code session — abort run | no |

The session matches against the structured error response (`code` + `details.detail`), applies the fix exactly once, retries the step once, then moves on. No fuzzy matching, no AI judgment.

### Resumability

When a session starts the runbook:
1. Scan `docs/runbook/runs/` for the most recent dir.
2. Read its `progress.jsonl`. If a `summary.md` exists, the previous run completed — create a new dir and start fresh.
3. If no summary: find the last completed step in `progress.jsonl`, log "resuming after <step id>", and continue from the next step.

A run dir is "in progress" if it has `progress.jsonl` but no `summary.md`.

### Concurrency

The runbook assumes one verification session at a time. The kde-mcp live lockfile enforces this at the kernel level — if a second session tries to spawn while one's already running, it gets `lock_unavailable` and the runbook aborts (per known-fixes table).

## What gets verified vs not

**Verified by this runbook:**
- All 16 tools end-to-end through the live MCP path.
- All schema-level expectations from `tests/integration/tools_list.rs`.
- The bug-bounty section of TESTING-CHECKLIST.md (deterministic items only).
- Most of TESTING-CHECKLIST.md's Phase 2 (D.1–D.5) and Phase 3 (policy gate) items.
- The 6 product scenarios.

**Not verified — requires human eyes:**
- Color fidelity of screenshots (TESTING-CHECKLIST.md "Manual UX gut-check").
- Typing speed feel.
- The KDE notification action-button UX (the runbook records that the notification fired; whether it *felt right* is subjective).
- Fractional-scaling correctness on mixed-DPI displays.
- Virtual mode (Phase 5) — explicit non-goal for v1; deferred until live mode is fully green.

These show up in the run summary as `needs_human(reason)` rather than pass/fail.

## Implementation cost

Pure documentation: 7 markdown files (≈600–800 lines total), one `.gitignore` line, no Rust changes.

| File | Lines (estimate) |
|---|---|
| `AUTONOMOUS-VERIFICATION.md` | 200 |
| `KNOWN-FIXES.md` | 50 |
| 6 × `scenarios/S*.md` | 6 × 60 = 360 |
| Spec doc (this file) | 250 |

The agent executing the runbook needs:
- Read access to `docs/runbook/`.
- Write access to `docs/runbook/runs/`.
- The `mcp__kde-mcp__*` tool family (already wired).
- `bash` access to query `xrandr`, `magick`, `pgrep`, `tail` on the audit log.

## Risks

1. **Runbook drift.** If kde-mcp gains a 17th tool, the L2 layer is silently incomplete. Mitigation: L1 runs `cargo test --workspace`, which fails on drift via `tests/integration/tools_list.rs::server_advertises_sixteen_tools_with_expected_names`. The runbook treats a cargo-test failure as `runbook_outdated` and aborts before continuing into L2.
2. **Brave dependency for S6.** If Brave is closed when the runbook starts, S6 is skipped — loss of one scenario, run continues.
3. **Approval-notification fatigue.** Destructive tool steps each fire a KDE policy-gate prompt the first time. After the first batch of approvals lands in `~/.local/state/kde-mcp/approvals.json` (Session-scoped), subsequent runs skip the prompts — but the first run requires ~6 clicks. Documented in the runbook intro so the user is prepared.
4. **Run artefacts may contain sensitive content.** Screenshot pixels, clipboard contents, file paths — gitignoring `runs/` mitigates leakage; the runbook intro warns the user not to share a `runs/` directory wholesale.

## Open questions

None blocking. Items deferred to future versions:

- v2: extend to Phase 5 (virtual mode) once it's live-verified.
- v2: add a `--strict` mode that fails the run on the first failure (current default is best-effort).
- v2: integration with the kde-mcp policy gate's audit log — automated diff against expected audit records.

## Approval gate

Before writing the runbook files, this design needs user sign-off on:
- The 4-layer structure.
- The 6 product scenarios chosen.
- The known-fixes table approach (vs an AI debug loop — explicit YAGNI for v1).
- The `runs/` directory being gitignored with no committed `HISTORY.md`.
