# Verification Runbook Implementation Plan

> Historical planning artifact — captures the plan as written on the date in the filename; the shipped implementation is the source of truth (see the ADRs and the design spec).

**Goal:** Produce a self-contained markdown runbook + log layout that lets a fresh agent session test `kde-mcp` end-to-end as a product, autonomously. Implements the design at `docs/specs/2026-05-15-verification-runbook-design.md`.

**Architecture:** Pure documentation. No Rust changes. Eight markdown files under `docs/runbook/`, one `.gitignore` line, and small updates to the README + `docs/TESTING-CHECKLIST.md` to reference the new runbook. The runbook is the "code" — every step uses a fixed parseable template (`Goal/Pre/Depends-on/Steps/Accept/Artefacts/On fail/Cleanup`) that an executing agent reads and follows uniformly. Auto-fix is a static `KNOWN-FIXES.md` lookup; no AI debug loop.

**Tech Stack:** Markdown (CommonMark). The agent that *runs* the runbook will use bash + `mcp__kde-mcp__*` tools + `python3` for JSON munging. No new dependencies for this plan.

---

## Spec map (so the engineer can audit coverage)

| Spec section | Implementing task(s) |
|---|---|
| Architecture / Layout | T1 |
| Layer 1 — Health & environment | T3 |
| Layer 2 — Per-tool acceptance | T4 |
| Layer 3 — Six product scenarios | T6–T11 |
| Layer 4 — Bug-bounty probes | T5 |
| Per-step template | T3 (defined once, referenced everywhere) |
| Known-fixes table | T2 |
| Resumability | T3 (resume protocol section) |
| Risks / non-goals | T3 (intro + risks section) |
| Cross-doc linking | T12 |
| Drift mitigation (cargo test in L1) | T3 |

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `.gitignore` | Modify | Add `docs/runbook/runs/` so per-run logs (potentially containing screenshots, clipboard contents, file paths) stay out of git. |
| `docs/runbook/AUTONOMOUS-VERIFICATION.md` | Create | Entry-point. Intro, how-to-run, resume protocol, Layer 1 (health), Layer 2 (16 tools), Layer 4 (bug-bounty B3/B4/B5/B6), summary template. |
| `docs/runbook/KNOWN-FIXES.md` | Create | Signature → mechanical fix lookup table. ~10 rows. |
| `docs/runbook/scenarios/S1-situational-awareness.md` | Create | observe(detail=interactive) sanity check. |
| `docs/runbook/scenarios/S2-drive-kcalc.md` | Create | bash launch → wait → keyboard.type "47*23=" → inspect display → assert 1081 → close. |
| `docs/runbook/scenarios/S3-ocr-fallback.md` | Create | Generate `~/.cache/kde-mcp-runbook/ocr-fixture.png` via `magick` → open in gwenview → read_text → assert substring. |
| `docs/runbook/scenarios/S4-clipboard-transport.md` | Create | Save clipboard → write probe → read → assert match → restore. |
| `docs/runbook/scenarios/S5-window-management.md` | Create | window_query → save geom → move to [100,100,800,600] → observe assert → restore. |
| `docs/runbook/scenarios/S6-brave-form-fill.md` | Create | window_query Brave → focus → ctrl+l → type marker → inspect URL bar → assert marker present → Esc. |
| `README.md` | Modify | Two-line "How to verify" pointer to the runbook. |
| `docs/TESTING-CHECKLIST.md` | Modify | Header note: items prefixed with `[auto]` are covered by the runbook; the rest still need human eyes. |

---

## Conventions used throughout this plan

- **No code blocks for content larger than ~30 lines.** Show the structure + one fully-worked example per task; describe the remaining items deterministically (so the engineer can mechanically extend without judgement calls).
- **Each task ends with a commit step.** Commit message uses project convention `type(scope): subject` (`docs(runbook): ...`).
- **No execution of the runbook itself in this plan.** That's a separate exercise once the runbook is written. (One smoke check on T13.)
- **Per-step template** (referenced by every Layer/Scenario step in T3–T11):

  ````markdown
  ### <ID> — <short title>
  **TC-Ref:** TESTING-CHECKLIST.md §<section> *(or `n/a` if no checklist mapping)*
  **Goal:** <one sentence>
  **Pre:** <preconditions; "none" if standalone>
  **Depends-on:** <other step ids, comma-separated; empty = independent>
  **Steps:**
  1. <action>
  2. <action>
  **Accept:** <structural assertions on the response>
  **Artefacts:** `runs/<id>/<file>` — what to save
  **On fail:**
    - <signature> → <fix from KNOWN-FIXES.md or "log and skip">
    - else: log full context to `runs/<id>/<ID>-failure.md`, mark `failed`, continue.
  **Cleanup:** <state to restore; "none" if idempotent>
  ````

---

## Task 1: Repo scaffold

**Files:**
- Modify: `.gitignore`
- Create: `docs/runbook/.keep` (empty file so `docs/runbook/` exists in git even when empty)
- Create: `docs/runbook/scenarios/.keep` (same)

- [ ] **Step 1: Add gitignore entry**

Edit `.gitignore` and append after the existing `# Runtime state` section:

```gitignore
# Runbook per-run artefacts (screenshots, audit excerpts, payloads).
# The runbook itself (docs/runbook/*.md + scenarios/*.md) is tracked.
docs/runbook/runs/
```

- [ ] **Step 2: Create the directory skeleton**

```bash
mkdir -p docs/runbook/scenarios
touch docs/runbook/.keep docs/runbook/scenarios/.keep
```

- [ ] **Step 3: Verify**

```bash
ls docs/runbook/
# expect: .keep  scenarios
ls docs/runbook/scenarios/
# expect: .keep
grep -F 'docs/runbook/runs/' .gitignore
# expect: one match
```

- [ ] **Step 4: Commit**

```bash
git add .gitignore docs/runbook/.keep docs/runbook/scenarios/.keep
git commit -m "docs(runbook): scaffold directory + gitignore runs/"
```

---

## Task 2: KNOWN-FIXES.md

**Files:**
- Create: `docs/runbook/KNOWN-FIXES.md`

- [ ] **Step 1: Write the file**

Path: `docs/runbook/KNOWN-FIXES.md`. Content:

````markdown
# Known fixes — signature → mechanical command

The runbook's executing agent consults this table when a step fails. **Apply at most one fix per failure**, retry the step **at most once**, then move on regardless of the retry's outcome. No fuzzy matching, no AI judgement — exact regex match against the failure's error code or stderr.

Matching priority: top-to-bottom; first match wins.

| # | Match (regex against `error.code` ∪ `stderr` ∪ `error.details.detail`) | Fix | Retry step once? | Notes |
|---|---|---|---|---|
| F1 | `binary not found at .*kde-mcp` OR `No such file or directory: .*\.local/bin/kde-mcp` | `./packaging/install.sh --skip-mcp` | yes | Rebuilds + re-installs the binary. Skips Claude config so we don't loop the install. |
| F2 | `not_authorized.*ScreenShot2` | `kbuildsycoca6 --noincremental` | yes | KApplicationTrader cache stale. Rebuild it; KWin will re-authorize on next call. |
| F3 | `busy` | (none) | yes | Another Claude Code session holds the mutation lease (ADR-0011). **Log and retry the dependent step once** — the leaseholder's burst is usually short. Persistent `busy` is logged + the step is marked failed. |
| F4 | `wl-paste: command not found` OR `wl-copy: command not found` | (none) | no | wl-clipboard not installed. Skip all S4 (clipboard transport) steps + L2 clipboard_read/clipboard_write. Log dep-missing. |
| F5 | `tesseract.*not found` OR `external_command_failed.*tesseract` | (none) | no | OCR backend missing. Skip S3. |
| F6 | `external_command_failed.*magick` OR `magick: command not found` | (none) | no | ImageMagick missing. Cannot generate S3 fixture. Skip S3. |
| F7 | `kde-mcp .* (connection refused|broken pipe|server crashed)` | (none) | no | MCP server died. **Abort the run.** Tell the user to restart their Claude Code session. |
| F8 | `app_approval_required` AND `still_pending: true` | (wait 30 s, then no fix — assume user clicked) | yes | Policy-gate prompt is in flight. The executing agent should sleep 30 s for the user to click Allow, then retry exactly once. If still pending, log and move on. |
| F9 | `cargo test` returns non-zero from L1.2 with `expected 16 wire tool names` | (none) | no | Runbook is **outdated** vs the binary's tool set. Abort the run and tell the user the runbook needs updating. |

**Anything not matching:** log the full failure response to `runs/<id>/<step-id>-failure.md`, mark the step `failed`, continue to the next independent step. The dependent steps will record `skipped(reason: dep <step-id> failed)`.
````

- [ ] **Step 2: Verify the table renders**

```bash
grep -c '^| F[0-9]' docs/runbook/KNOWN-FIXES.md
# expect: 9
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/KNOWN-FIXES.md
git commit -m "docs(runbook): KNOWN-FIXES.md — signature → fix table"
```

---

## Task 3: AUTONOMOUS-VERIFICATION.md skeleton + Layer 1

**Files:**
- Create: `docs/runbook/AUTONOMOUS-VERIFICATION.md`

This task creates the file with intro, how-to-run, resume protocol, Layer 1 (health & environment), and stub sections for Layers 2 and 4. Tasks 4 and 5 fill those stubs.

- [ ] **Step 1: Write the file shell**

Path: `docs/runbook/AUTONOMOUS-VERIFICATION.md`. Use this exact structure:

````markdown
# Autonomous verification runbook — kde-mcp

A fresh agent session should be able to verify kde-mcp end-to-end by reading this file top-to-bottom and executing each step. Auto-fix is mechanical only — see `KNOWN-FIXES.md`. There is no AI debug loop; novel failures get logged and the run continues.

**Design spec:** `docs/specs/2026-05-15-verification-runbook-design.md`.

---

## How to run (for the executing agent)

1. Choose a run ID: `run_id="$(date +%Y-%m-%d-%H%M)"`.
2. `mkdir -p docs/runbook/runs/$run_id/audit-excerpts`.
3. Open a new `progress.jsonl` at `docs/runbook/runs/$run_id/progress.jsonl`. Write one line per step as `{"id":"L1.1","status":"started","ts":"<ISO8601>"}` then `{"id":"L1.1","status":"<passed|failed|skipped>","ts":"<ISO8601>","notes":"..."}` when done.
4. Snapshot the environment to `runs/$run_id/env.json` (see Layer 1, step L1.6).
5. Execute Layer 1, then 2, then 3, then 4 in order. Skip steps whose `Depends-on` lists a `failed` step.
6. When done, write `runs/$run_id/summary.md` using the template at the bottom of this file.

**Per-step template:**

```
### <ID> — <title>
**TC-Ref:** TESTING-CHECKLIST.md §<section> or n/a
**Goal:** one sentence
**Pre:** preconditions
**Depends-on:** step ids
**Steps:** ordered list of tool calls
**Accept:** structural assertions
**Artefacts:** files to save under runs/$run_id/
**On fail:** signature → fix or log-and-skip
**Cleanup:** state to restore
```

The signature-match for `On fail` consults `KNOWN-FIXES.md`. Anything not matching: log + continue.

## Resume protocol

If `docs/runbook/runs/` has a directory whose `progress.jsonl` exists but no `summary.md`:
1. Read its `progress.jsonl`. Find the last entry whose `status != "started"`.
2. Reuse that run dir (don't create a new one).
3. Log a step `RESUME` with `notes: "resuming after <last completed step id>"`.
4. Continue from the next undone step.

If no in-progress run exists, create a fresh dir.

---

## Layer 1 — Health & environment

Cheap, no live side-effects. Aborts the run on failure.

### L1.1 — Installed binary present
**TC-Ref:** n/a
**Goal:** Verify `~/.local/bin/kde-mcp` exists and is executable.
**Pre:** none.
**Depends-on:** (none)
**Steps:**
1. `bash`: `test -x ~/.local/bin/kde-mcp && stat -c '%s %y' ~/.local/bin/kde-mcp`
**Accept:** command exits 0; output is a size + timestamp.
**Artefacts:** `L1.1-binary.txt` (stdout of stat).
**On fail:** F1 (run `./packaging/install.sh --skip-mcp`, retry once). Otherwise abort run.
**Cleanup:** none.

### L1.2 — Cargo test suite green
**TC-Ref:** n/a
**Goal:** Hermetic tests pass; in particular, the 16-tool wire-name assertion holds.
**Pre:** Repo working tree clean (else log and continue with warning).
**Depends-on:** L1.1
**Steps:**
1. `bash`: `cargo test --workspace 2>&1 | tail -10`
**Accept:** Each of the four `test result` lines shows `ok. .* 0 failed`. Total: 110+ passing.
**Artefacts:** `L1.2-cargo-test.log` (full output).
**On fail:** if output contains `expected 16 wire tool names` → F9 (abort, runbook outdated). Else log full output, mark failed, **abort run** — there's no point verifying live behavior of broken code.
**Cleanup:** none.

### L1.3 — .desktop file authorises ScreenShot2
**TC-Ref:** n/a
**Goal:** KApplicationTrader will find the binary's screenshot authorisation.
**Pre:** L1.1 passed.
**Depends-on:** L1.1
**Steps:**
1. `bash`: `cat ~/.local/share/applications/kde-mcp.desktop`
**Accept:** Contains both `Exec=` pointing at the same path as L1.1 and the line `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`.
**Artefacts:** `L1.3-desktop.txt`.
**On fail:** F2 (kbuildsycoca6), retry once. Else log + continue (screenshot steps will likely fail downstream).
**Cleanup:** none.

### L1.4 — MCP server registered and connected
**TC-Ref:** n/a
**Goal:** Claude Code sees `kde-mcp` and it's `Connected`.
**Pre:** none.
**Depends-on:** L1.1
**Steps:**
1. `bash`: `claude mcp list | grep -E '^kde-mcp'`
**Accept:** Output contains `kde-mcp:` and `✓ Connected`.
**Artefacts:** `L1.4-mcp-list.txt`.
**On fail:** F7 if "connection refused" / "server crashed". Else log + continue (subsequent steps may still work if the server is starting up).
**Cleanup:** none.

### L1.5 — Single live kde-mcp process
**TC-Ref:** n/a
**Goal:** No duplicate / stale processes holding the live lock.
**Pre:** none.
**Depends-on:** (none)
**Steps:**
1. `bash`: `pgrep -af kde-mcp | grep -v -E '(install|runbook)' | wc -l`
**Accept:** Exactly `1` (the one Claude Code spawned).
**Artefacts:** `L1.5-pgrep.txt`.
**On fail:** if `> 1`, list PIDs and log — F3 abort if user can't kill the duplicate. If `0`, the MCP server isn't running — F7 abort.
**Cleanup:** none.

### L1.6 — Environment snapshot
**TC-Ref:** n/a
**Goal:** Record host versions so future readers of `runs/$run_id/` know what was tested.
**Pre:** none.
**Depends-on:** (none)
**Steps:**
1. `bash`: collect outputs of:
   - `plasmashell --version` (one line)
   - `kwin_wayland --version` (one line)
   - `xrandr --listmonitors` (multiple lines)
   - `kde-mcp --version 2>/dev/null || echo unknown` (one line)
   - `git -C ~/repositories/ns/atassis/kde-mcp rev-parse --short HEAD`
2. Combine into a JSON object: `{plasma: "...", kwin: "...", monitors: [...], kde_mcp_version: "...", repo_sha: "..."}`.
3. Write to `runs/$run_id/env.json`.
**Accept:** File exists, parses as valid JSON, has all five keys.
**Artefacts:** `env.json` (already named).
**On fail:** log the missing tool; record the JSON with `null` for the unreachable fields. Don't abort.
**Cleanup:** none.

---

## Layer 2 — Per-tool acceptance

*(Filled in by Task 4 of the implementation plan.)*

## Layer 3 — Product scenarios

*(Each is a separate file under `scenarios/`. Run them in numerical order.)*

| ID | File | Pre-condition |
|---|---|---|
| S1 | `scenarios/S1-situational-awareness.md` | At least one window open. |
| S2 | `scenarios/S2-drive-kcalc.md` | KCalc installed (`which kcalc`). |
| S3 | `scenarios/S3-ocr-fallback.md` | `magick`, `gwenview`, `tesseract` installed. |
| S4 | `scenarios/S4-clipboard-transport.md` | `wl-clipboard` installed. |
| S5 | `scenarios/S5-window-management.md` | At least one user-focusable window open. |
| S6 | `scenarios/S6-brave-form-fill.md` | Brave running with at least one tab. |

## Layer 4 — Bug-bounty probes

*(Filled in by Task 5 of the implementation plan.)*

---

## Summary template

The executing agent writes `runs/$run_id/summary.md` at end of run with this structure:

```markdown
# Run summary — $run_id

**Started:** <ISO8601>
**Ended:** <ISO8601>
**Repo SHA:** <from env.json>
**KDE Plasma:** <from env.json>

## Headline
<passed>/<total> steps passed (<failed> failed, <skipped> skipped).

## Failures
<one entry per failed step: id, what happened, link to runs/$run_id/<id>-failure.md>

## Skipped
<id, reason>

## Needs human eyes
<id, what to look at — color fidelity, typing feel, etc.>

## Regressions vs previous run
<diff against the most recent prior run's summary.md, if any>
```
````

- [ ] **Step 2: Verify**

```bash
wc -l docs/runbook/AUTONOMOUS-VERIFICATION.md
# expect: 150 < L < 250 (skeleton — fills out in T4, T5)
grep -c '^### L1\.' docs/runbook/AUTONOMOUS-VERIFICATION.md
# expect: 6
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/AUTONOMOUS-VERIFICATION.md
git commit -m "docs(runbook): AUTONOMOUS-VERIFICATION.md skeleton + Layer 1"
```

---

## Task 4: Layer 2 — fill in 16 per-tool acceptance steps

**Files:**
- Modify: `docs/runbook/AUTONOMOUS-VERIFICATION.md` — replace the `*(Filled in by Task 4 of the implementation plan.)*` stub with 16 step blocks.

- [ ] **Step 1: Plan the 16 entries**

Order — read-only first (no destructive policy-gate prompts), destructive last:

| ID | Tool | Probe | Acceptance |
|---|---|---|---|
| L2.1 | `observe` | `{detail: "summary"}` | `windows.length >= 1`, `focused_window` non-null, no `error`. |
| L2.2 | `inspect` | `{target: "focused"}` | response has `elements` array (may be empty if focus tracker not started — still passes); no `error`. |
| L2.3 | `screenshot` | `{region: "active_screen"}` | response has `image.png` (base64 or resource_link) and `dims: [w, h]` with both > 0. |
| L2.4 | `read_text` | `{region: "active_screen", lang: "eng"}` | response has `text` (may be empty); no panic. |
| L2.5 | `wait` | `{condition: "seconds", seconds: 0.2}` | `matched: true`, `elapsed_ms` in `[150, 400]`. |
| L2.6 | `window_query` | `{action: "list"}` | response has `windows` array, length matches L2.1's count (±1 for race). |
| L2.7 | `clipboard_read` | `{}` | response has `content` field (possibly empty string); no `error` other than `target_not_found` (empty clipboard). |
| L2.8 | `text_editor.view` | `{path: "/etc/hostname"}` | response contains the host's hostname (= `bash` `hostname` output). |
| L2.9 | `clipboard_write` | `{content: "kde-mcp-probe-$run_id"}` | `ok: true`. Sentinel for L2.10. |
| L2.10 | `clipboard_read` (verify L2.9) | `{}` | content equals probe from L2.9. |
| L2.11 | `keyboard.key` | `{chord: "F12"}` against focused window | no `error`. (F12 is mostly inert — debug toggle.) |
| L2.12 | `keyboard.type` | `{text: "kdemcp"}` against focused window | no `error`; `chars_emitted: 6`. |
| L2.13 | `mouse` | `{action: "move", x: 100, y: 100}` | no `error`. (Move only; no click.) |
| L2.14 | `act_on_element` | depends on L2.2 returning an element with `actions` non-empty | no `error`. **If L2.2's response has zero actionable elements, skip with `skipped(reason: no actionable element)`.** |
| L2.15 | `bash` | `{command: "echo hello", restart: false}` | response has `stdout: "hello\n"`, `exit_code: 0`. |
| L2.16 | `window_control` | `{action: "focus", window_uuid: <L2.6's first window>}` | no `error`. |
| L2.17 | `text_editor.edit` | `{command: "create", path: "/tmp/kde-mcp-runbook-probe.txt", file_text: "probe"}` | `ok: true`. Cleanup: `rm /tmp/kde-mcp-runbook-probe.txt` via bash. |

(17 entries because L2.10 is a verify-of-L2.9 — they're logically paired. The L2 layer has 16 *distinct tools* but 17 steps. Number them L2.1 through L2.17.)

- [ ] **Step 2: Write one fully-worked example, then mechanically produce the rest**

In `docs/runbook/AUTONOMOUS-VERIFICATION.md`, replace the `*(Filled in by Task 4 of the implementation plan.)*` placeholder under `## Layer 2 — Per-tool acceptance` with:

````markdown
Each step calls one tool via the live MCP session, validates the response structurally (not exact-match — KDE state drifts), saves the response payload to `runs/$run_id/L2.<n>-<tool>.json`. Read-only tools first (no policy-gate prompts), destructive last.

### L2.1 — observe(detail=summary)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke" (D.1 adjacent)
**Goal:** Prove the AT-SPI + KWin script roundtrip works end-to-end via MCP.
**Pre:** ≥1 window on the desktop.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__observe` with `{"detail":"summary"}`.
2. Save the raw response (the MCP tool result block) to `runs/$run_id/L2.1-observe.json`.
**Accept:** `result.windows` array length ≥ 1; `result.focused_window` is non-null (an object with at least `uuid` and `class`); no top-level `error`.
**Artefacts:** `L2.1-observe.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.2 — inspect(target=focused)
*(Same template — fill in per the table above.)*

### L2.3 — screenshot(region=active_screen)
*(Same.)*

[…L2.4 through L2.17, one block each, following the same template…]
````

The engineer fills in steps L2.2 through L2.17 following the same shape using the probe/acceptance table above. For each:
- Goal: paraphrase from the tool's `#[tool(description=...)]` line in `src/server.rs`.
- TC-Ref: cross-reference into TESTING-CHECKLIST.md — most entries map to §"Phase 2 live smoke", §"Phase 4 — remaining destructive tools", or §"Cross-cutting / non-functional".
- Depends-on: `L1.4` for tools that need the live MCP session (all of them). Plus L2.9 for L2.10. Plus L2.2 for L2.14. Plus L2.6 for L2.16.
- Pre: only flag preconditions that are non-obvious (e.g., L2.7 needs wl-clipboard).
- On fail: always check KNOWN-FIXES first; F4 for clipboard, F7 for connection errors. Else log+continue.
- Cleanup: L2.9 leaves clipboard polluted (S4 will restore later, but if S4 doesn't run, leave a note); L2.17 must `rm /tmp/kde-mcp-runbook-probe.txt` via `bash`.

- [ ] **Step 3: Verify all 17 step IDs are present**

```bash
grep -c '^### L2\.' docs/runbook/AUTONOMOUS-VERIFICATION.md
# expect: 17
```

- [ ] **Step 4: Verify each tool from EXPECTED_WIRE_NAMES appears**

```bash
for tool in observe inspect screenshot read_text wait window_query window_control clipboard_read clipboard_write text_editor.view text_editor.edit keyboard.type keyboard.key mouse act_on_element bash; do
    grep -qF "$tool" docs/runbook/AUTONOMOUS-VERIFICATION.md || echo "MISSING: $tool"
done
# expect: no output (all tools covered)
```

- [ ] **Step 5: Commit**

```bash
git add docs/runbook/AUTONOMOUS-VERIFICATION.md
git commit -m "docs(runbook): Layer 2 — 17 per-tool acceptance steps"
```

---

## Task 5: Layer 4 — bug-bounty probes B3/B4/B5/B6

**Files:**
- Modify: `docs/runbook/AUTONOMOUS-VERIFICATION.md` — replace the `*(Filled in by Task 5 of the implementation plan.)*` placeholder under `## Layer 4 — Bug-bounty probes`.

- [ ] **Step 1: Write the section**

Replace the placeholder with:

````markdown
A subset of `docs/TESTING-CHECKLIST.md §"Quick-fire bug-bounty list"`. Only the externally-observable items are automated here. B1 (shift-release-between-caps) and B2 (mouse double-click coalescing) are deferred to v2 — they need EIS-frame instrumentation that doesn't yet exist.

### L4.B3 — clipboard binary round-trip
**TC-Ref:** TESTING-CHECKLIST.md §"Quick-fire bug-bounty list" line 176
**Goal:** Prove `clipboard_write` with `encoding=base64` decodes server-side, not just stores the literal base64 string.
**Pre:** `wl-clipboard` installed (else F4 skip).
**Depends-on:** L1.4
**Steps:**
1. `bash`: Generate a deterministic 16-byte binary input (includes NUL + high-bit bytes that would be mangled by any text-mode handling):

   ```sh
   printf 'kde-mcp\x00binary\xff\x7f' > /tmp/runbook-b3-input.bin
   base64 /tmp/runbook-b3-input.bin
   ```

   Record the base64 output as `<b64>` and the sha256 of the raw input as `<sha256_in>` (via `sha256sum /tmp/runbook-b3-input.bin`).
2. Call `mcp__kde-mcp__clipboard_write` with `{"content": "<b64>", "encoding": "base64"}`.
3. Call `mcp__kde-mcp__clipboard_read` with `{}`. Save full response to `runs/$run_id/L4.B3-readback.json`.
4. `bash`: extract the response's content (it may be a string or base64; check the response's `encoding` field). Decode if needed and write the bytes to `/tmp/runbook-b3-readback.bin`. Compute `sha256_out` via `sha256sum`.
**Accept:** `sha256_out == sha256_in` (16 bytes round-tripped byte-equal).
**Artefacts:** `L4.B3-input.b64`, `L4.B3-readback.json`, `L4.B3-result.txt` (PASS/FAIL with both sha256 hashes).
**On fail:** signature match against KNOWN-FIXES (F4 for clipboard). Else log + continue.
**Cleanup:** `clipboard_write` an empty string to clear.

### L4.B4 — multi-monitor screenshot region (CONDITIONAL)
**TC-Ref:** TESTING-CHECKLIST.md §"Quick-fire bug-bounty list" line 177
**Goal:** Verify a region crossing monitor seams returns coherent pixels.
**Pre:** `env.json` shows `>= 2` monitors. Else skip with `skipped(precondition: single monitor)`.
**Depends-on:** L1.6, L1.4
**Steps:**
1. Parse `runs/$run_id/env.json` for monitor geometries.
2. If only one monitor: write `skipped` to progress.jsonl and stop.
3. Compute a region rectangle that straddles the seam between the first two monitors (e.g., from monitor-1's right edge minus 100 px, spanning 200 px wide, full height of monitor-1).
4. Call `mcp__kde-mcp__screenshot` with `{"region": [x, y, w, h]}` using the computed values.
5. Save the returned PNG (or resource_link) to `runs/$run_id/L4.B4-screenshot.png`.
**Accept:** PNG dimensions match the requested width/height. Both halves of the image (left of seam, right of seam) contain non-uniform pixel data (basic sanity — neither half is all-black).
**Artefacts:** `L4.B4-screenshot.png`, `L4.B4-result.txt` (notes for human eyes).
**On fail:** F2 (kbuildsycoca6) for `not_authorized`. Else log + continue.
**Cleanup:** none.

### L4.B5 — inspect deep tree
**TC-Ref:** TESTING-CHECKLIST.md §"Quick-fire bug-bounty list" line 178
**Goal:** Verify `inspect max_elements=10000` against a deep app returns in bounded time without crash.
**Pre:** Dolphin installed (`which dolphin`). Else skip.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__bash` with `{"command": "dolphin &", "restart": false}` (background-launch).
2. Call `mcp__kde-mcp__wait` with `{"condition": "window_appears", "match": {"class": "org.kde.dolphin"}, "timeout_ms": 5000}`.
3. From the wait response, grab the window UUID.
4. Record start time. Call `mcp__kde-mcp__inspect` with `{"target": "window:<uuid>", "max_elements": 10000}`. Record end time.
5. Save response to `runs/$run_id/L4.B5-inspect.json`.
**Accept:** Call returns within 5 seconds. Response has an `elements` array (`truncated: true` OR `truncated: false` — both acceptable). No timeout / crash.
**Artefacts:** `L4.B5-inspect.json`, `L4.B5-timing.txt`.
**On fail:** log + continue.
**Cleanup:** `window_control action=close window_uuid=<uuid>`.

### L4.B6 — wait screen_change on static screen
**TC-Ref:** TESTING-CHECKLIST.md §"Quick-fire bug-bounty list" line 179
**Goal:** `wait condition=screen_change` against a static screen times out cleanly rather than blocking forever.
**Pre:** Don't move the mouse or trigger animations during this step.
**Depends-on:** L1.4
**Steps:**
1. Record start time.
2. Call `mcp__kde-mcp__wait` with `{"condition": "screen_change", "pixel_threshold": 0.05, "timeout_ms": 500}`.
3. Record end time.
4. Save response.
**Accept:** Response has `matched: false`. Elapsed time in `[400, 800] ms`. Error code (if any) is `timeout`.
**Artefacts:** `L4.B6-wait.json`, `L4.B6-timing.txt`.
**On fail:** log + continue.
**Cleanup:** none.

---

### Deferred to v2
- **B1** — shift release between capital letters in `keyboard.type`. Requires `RUST_LOG=trace` capture of EIS frames; not externally observable.
- **B2** — mouse double-click coalescing by `qApp->doubleClickInterval()`. Needs a target app's event log or a custom probe widget.

Both are documented in `docs/TESTING-CHECKLIST.md` and will be picked up when EIS-frame instrumentation lands.
````

- [ ] **Step 2: Verify**

```bash
grep -c '^### L4\.' docs/runbook/AUTONOMOUS-VERIFICATION.md
# expect: 4
grep -c 'Deferred to v2' docs/runbook/AUTONOMOUS-VERIFICATION.md
# expect: 1
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/AUTONOMOUS-VERIFICATION.md
git commit -m "docs(runbook): Layer 4 — bug-bounty B3/B4/B5/B6 + v2-deferred note"
```

---

## Task 6: Scenario S1 — Situational awareness

**Files:**
- Create: `docs/runbook/scenarios/S1-situational-awareness.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S1 — Situational awareness

> "What's on my screen right now?"

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke" (broad)
**Goal:** Single `observe(detail=interactive)` call returns a coherent snapshot. Smoke for the user-facing entry point Claude would call first when given a vague desktop task.
**Pre:** ≥1 window open on the desktop.
**Depends-on:** L2.1 (basic observe). If L2.1 failed, skip this scenario.

## Steps
1. Call `mcp__kde-mcp__observe` with `{"detail":"interactive"}`.
2. Save response to `runs/$run_id/S1-observe.json`.
3. Parse and record:
   - `windows.length`
   - `focused_window` (or `null`)
   - `screens.length`
   - `cursor` (object with `x`, `y`)
   - `warnings[]` (collect)

## Accept
- `windows.length >= 1`
- `focused_window` is non-null and has `uuid`, `class`, `title` fields (title may be empty string)
- `screens.length >= 1`, matches `env.json.monitors.length`
- No top-level `error`

## Artefacts
- `runs/$run_id/S1-observe.json` — raw response
- `runs/$run_id/S1-result.md` — short PASS/FAIL summary with the parsed fields above

## On fail
- F7 if connection error
- Else log + mark `failed`. Skip S2–S6 dependents (none depend on S1 strictly, but if `observe` is broken the others probably are too).

## Cleanup
None.
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S1-situational-awareness.md && wc -l docs/runbook/scenarios/S1-situational-awareness.md
# expect: 30 < L < 70
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S1-situational-awareness.md
git commit -m "docs(runbook): scenario S1 — situational awareness"
```

---

## Task 7: Scenario S2 — Drive KCalc

**Files:**
- Create: `docs/runbook/scenarios/S2-drive-kcalc.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S2 — Drive KCalc end-to-end

> "Open KCalc, compute 47 × 23, return the result."

The classic GUI-automation demo. Exercises: bash launch + wait + window_control focus + keyboard.type + inspect + window_control close. End-to-end product flow.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Drive KCalc to compute 47*23, confirm display reads 1081.
**Pre:** `kcalc` installed. If not: skip with `skipped(precondition: kcalc not installed)`.
**Depends-on:** L2.15 (bash), L2.5 (wait), L2.16 (window_control focus), L2.12 (keyboard.type), L2.2 (inspect).

## Steps
1. Check `kcalc` is available: `bash "command -v kcalc"`. If exit non-zero, skip.
2. Launch: `bash "kcalc &"`.
3. Wait for window: `mcp__kde-mcp__wait` with `{"condition":"window_appears", "match":{"class":"org.kde.kcalc"}, "timeout_ms":5000}`. Grab `window_uuid` from response.
4. Focus: `mcp__kde-mcp__window_control` with `{"action":"focus", "window_uuid":"<uuid>"}`.
5. Type input: `mcp__kde-mcp__keyboard.type` with `{"text":"47*23"}`.
6. Press Enter: `mcp__kde-mcp__keyboard.key` with `{"chord":"Return"}`.
7. Inspect the display: `mcp__kde-mcp__inspect` with `{"target":"window:<uuid>"}`. Filter elements where `role` matches `Text|StaticText|LCD|Display` (KCalc's display element varies by version — collect candidates).
8. Save inspect response to `runs/$run_id/S2-inspect.json`.
9. Pick the element whose `value` looks numeric.

## Accept
- One of the inspected elements has `value` (or `name`) containing `1081` or `1,081`.
- All tool calls returned `ok: true`.

## Artefacts
- `runs/$run_id/S2-wait.json`
- `runs/$run_id/S2-inspect.json`
- `runs/$run_id/S2-result.md` with: input → result, PASS/FAIL, the chosen display element's role+name+value.

## On fail
- `app_approval_required` for kcalc → F8 (wait 30s for user click, retry once).
- `target_not_found` from wait → log "kcalc didn't appear", mark failed, skip cleanup.
- No element with numeric value → log all inspected elements' values to `S2-result.md`, mark failed.
- Otherwise: log + continue.

## Cleanup
`mcp__kde-mcp__window_control` with `{"action":"close", "window_uuid":"<uuid>"}`. If KCalc prompts to save (it shouldn't for a fresh calc), press Escape via `keyboard.key`.
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S2-drive-kcalc.md && grep -c '## ' docs/runbook/scenarios/S2-drive-kcalc.md
# expect: 6 (top heading not counted; Steps, Accept, Artefacts, On fail, Cleanup + headings inside)
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S2-drive-kcalc.md
git commit -m "docs(runbook): scenario S2 — drive KCalc"
```

---

## Task 8: Scenario S3 — OCR fallback

**Files:**
- Create: `docs/runbook/scenarios/S3-ocr-fallback.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S3 — OCR fallback

> "Read text from a window where AT-SPI doesn't expose it (canvas / PDF / image)."

Generates a known-text image on first run, opens it in an image viewer, runs `read_text` over the window. Tests the vision-fallback path. Output is `trust: external` per spec §4.7.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools" / read_text
**Goal:** OCR a fixture image and confirm the recognised text matches the embedded probe string.
**Pre:** `magick` (ImageMagick), `gwenview` (or any image viewer), `tesseract` with `tesseract-data-eng` installed.
**Depends-on:** L2.15 (bash), L2.5 (wait), L2.4 (read_text — at least sanity-passed).

## Steps
1. Check deps: `bash "command -v magick && command -v gwenview && command -v tesseract"`. If any missing, skip.
2. Generate fixture (idempotent — only if missing): `bash 'mkdir -p ~/.cache/kde-mcp-runbook && [ -f ~/.cache/kde-mcp-runbook/ocr-fixture.png ] || magick -background white -fill black -size 600x150 -gravity center caption:"kde-mcp ocr probe ${run_id}" ~/.cache/kde-mcp-runbook/ocr-fixture.png'`. (Replace `${run_id}` in the caption call with the actual run id string before passing to bash so the image content is deterministic per run.)
3. Open in gwenview: `bash "gwenview ~/.cache/kde-mcp-runbook/ocr-fixture.png &"`.
4. Wait for window: `wait` with `{"condition":"window_appears","match":{"class":"org.kde.gwenview"},"timeout_ms":5000}`.
5. Call `mcp__kde-mcp__read_text` with `{"region":"window:<uuid from wait>", "lang":"eng"}`.
6. Save response to `runs/$run_id/S3-readtext.json`.

## Accept
- Response's `text` field contains the substring `kde-mcp ocr probe` (case-insensitive). Trailing OCR noise is allowed.
- Response's `trust` field is `external`.

## Artefacts
- `runs/$run_id/S3-readtext.json`
- `runs/$run_id/S3-result.md` with PASS/FAIL + the recognised text verbatim
- (Fixture lives outside the run dir at `~/.cache/kde-mcp-runbook/ocr-fixture.png` — durable across runs)

## On fail
- F5 (tesseract not found) → skip.
- F6 (magick not found) → skip.
- OCR returned but no probe substring → log the actual OCR text to `S3-result.md`, mark failed.
- Otherwise: log + continue.

## Cleanup
Close gwenview via `window_control action=close`. Fixture left in cache (intentional — reused across runs).
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S3-ocr-fallback.md
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S3-ocr-fallback.md
git commit -m "docs(runbook): scenario S3 — OCR fallback with generated fixture"
```

---

## Task 9: Scenario S4 — Clipboard transport

**Files:**
- Create: `docs/runbook/scenarios/S4-clipboard-transport.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S4 — Clipboard as transport

> "Copy this for me / paste what's on the clipboard / preserve what was there before."

Round-trips a probe string through the clipboard, then restores the user's prior content. Tests the round-trip + the "no leak" requirement that `keyboard.type` for non-ASCII relies on internally.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke" D.2 + D.5
**Goal:** Write → read → restore round-trip with no clipboard leak.
**Pre:** `wl-clipboard` installed (else F4 skip whole scenario).
**Depends-on:** L2.7 (clipboard_read), L2.9 (clipboard_write).

## Steps
1. Save current clipboard: `mcp__kde-mcp__clipboard_read` with `{}`. Store the response's content as `original` in your local variables. Save to `runs/$run_id/S4-original.json`.
2. Write probe: `mcp__kde-mcp__clipboard_write` with `{"content":"kde-mcp-probe-${run_id}"}`.
3. Read back: `mcp__kde-mcp__clipboard_read` with `{}`. Save to `runs/$run_id/S4-readback.json`.
4. Restore: `mcp__kde-mcp__clipboard_write` with `{"content":"<original>"}`. (If `<original>` is empty, write a single space then warn — wl-copy doesn't accept truly empty content gracefully on some setups.)
5. Verify restore: `mcp__kde-mcp__clipboard_read` with `{}`. Save to `runs/$run_id/S4-restored.json`.

## Accept
- Step 3's content equals `kde-mcp-probe-${run_id}` (exact match).
- Step 5's content equals step 1's content (round-trip preserves user state).
- No tool call returned an error other than `target_not_found` (legitimate empty clipboard at start).

## Artefacts
- `runs/$run_id/S4-original.json`
- `runs/$run_id/S4-readback.json`
- `runs/$run_id/S4-restored.json`
- `runs/$run_id/S4-result.md` with the three diffs

## On fail
- F4 if clipboard tooling missing.
- Round-trip mismatch on step 3 → likely a kde-mcp bug in `clipboard_write` encoding. Log full payloads.
- Restore mismatch on step 5 → clipboard leak; flag prominently in summary (security-relevant).
- Otherwise: log + continue.

## Cleanup
None — step 4 already restored.
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S4-clipboard-transport.md
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S4-clipboard-transport.md
git commit -m "docs(runbook): scenario S4 — clipboard transport round-trip"
```

---

## Task 10: Scenario S5 — Window management

**Files:**
- Create: `docs/runbook/scenarios/S5-window-management.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S5 — Window management round-trip

> "Move this window to the right half of my screen."

Pick the focused window, save its geometry, move it to a fixed rectangle, verify the move, restore. Tests `window_query` → `window_control(action=move)` → restore.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools" / window_control
**Goal:** Move + restore a window via the API with no user-visible state loss.
**Pre:** ≥1 user-focusable window open. Avoid running this when the focused window is a video / fullscreen app.
**Depends-on:** L2.6 (window_query), L2.16 (window_control).

## Steps
1. `mcp__kde-mcp__window_query` with `{"action":"list","format":"full"}`. Save to `runs/$run_id/S5-before.json`.
2. Pick the focused window from the response (`focused: true` flag, or fall back to first non-system window).
3. Record its `geometry` (x, y, w, h) and `uuid` as `target`.
4. `mcp__kde-mcp__window_control` with `{"action":"move","window_uuid":"<target.uuid>","geometry":[100,100,800,600]}`.
5. `mcp__kde-mcp__observe` with `{"detail":"summary"}`. Save to `runs/$run_id/S5-after-move.json`.
6. Find the same window (by uuid) in observe's response and read its new geometry.
7. Restore: `mcp__kde-mcp__window_control` with `{"action":"move","window_uuid":"<target.uuid>","geometry":<target.geometry>}`.
8. Verify restore: `mcp__kde-mcp__observe` again. Save to `runs/$run_id/S5-after-restore.json`.

## Accept
- After step 5: target window's geometry equals `[100, 100, 800, 600]` (±10 px for window decoration handling).
- After step 8: target window's geometry equals the original (±10 px).
- No errors.

## Artefacts
- `runs/$run_id/S5-before.json`, `S5-after-move.json`, `S5-after-restore.json`
- `runs/$run_id/S5-result.md` with the before/after geometry triple

## On fail
- `app_approval_required` → F8.
- Move silently no-op (geometry unchanged after step 4) → likely a window-manager-side restriction (tiled / maximised window). Log and mark `concerning` (not failed) — this is a real KDE behaviour, not a kde-mcp bug.
- Restore mismatch → kde-mcp bug. Mark failed.
- Otherwise: log + continue.

## Cleanup
Already restored in step 7. If step 7 errored, log a clear "restoration FAILED — manual cleanup needed" entry in summary.md.
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S5-window-management.md
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S5-window-management.md
git commit -m "docs(runbook): scenario S5 — window management round-trip"
```

---

## Task 11: Scenario S6 — Brave form-fill

**Files:**
- Create: `docs/runbook/scenarios/S6-brave-form-fill.md`

- [ ] **Step 1: Write the file**

Content:

````markdown
# S6 — Brave form-fill

> "Type something into the URL bar so I can see it ended up where I asked."

Tests AT-SPI introspection + keyboard.type on a real browser. Doesn't actually navigate — we want to verify text *arrived in the focused widget*, not that Brave loaded a page.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Type a marker string into Brave's URL bar, then read it back via AT-SPI, confirming end-to-end text delivery.
**Pre:** Brave running with at least one tab. If not running, skip.
**Depends-on:** L2.6 (window_query), L2.16 (window_control), L2.11 (keyboard.key), L2.12 (keyboard.type), L2.2 (inspect).

## Steps
1. `mcp__kde-mcp__window_query` with `{"action":"list","format":"full"}`. Save to `runs/$run_id/S6-windows.json`.
2. Find a window whose `class` contains `brave-browser` (case-insensitive). Record its `uuid`. If none: skip with `skipped(precondition: Brave not running)`.
3. `mcp__kde-mcp__window_control` with `{"action":"focus","window_uuid":"<uuid>"}`.
4. `mcp__kde-mcp__wait` with `{"condition":"seconds","seconds":0.3}` (let focus settle).
5. `mcp__kde-mcp__keyboard.key` with `{"chord":"ctrl+l"}` (focus address bar; Brave's URL bar shortcut).
6. `mcp__kde-mcp__wait` with `{"condition":"seconds","seconds":0.2}`.
7. `mcp__kde-mcp__keyboard.type` with `{"text":"kde-mcp-marker-${run_id}"}`.
8. `mcp__kde-mcp__wait` with `{"condition":"seconds","seconds":0.3}` (let AT-SPI settle).
9. `mcp__kde-mcp__inspect` with `{"target":"window:<uuid>"}`. Save to `runs/$run_id/S6-inspect.json`.
10. Search the inspect response for elements where `role` is `entry`, `text`, `textbox`, or similar AND `value` contains the marker substring `kde-mcp-marker-${run_id}`.

## Accept
- At least one inspected element matches: marker text is present in an AT-SPI text/entry element's value.
- No top-level errors.

## Artefacts
- `runs/$run_id/S6-windows.json`
- `runs/$run_id/S6-inspect.json`
- `runs/$run_id/S6-result.md` with: marker string, matched element role/value, PASS/FAIL

## On fail
- `app_approval_required` for Brave → F8.
- Marker absent from inspect → check screenshot manually; might be Brave-specific AT-SPI quirk. Log all entry-like elements' values to S6-result.md.
- F4 if any clipboard error (the Cyrillic clipboard fallback shouldn't trigger here — ASCII marker — but log if it does).
- Otherwise: log + continue.

## Cleanup
- `mcp__kde-mcp__keyboard.key` with `{"chord":"Escape"}` to clear the URL bar without navigating.
- Don't press Return — we don't want to actually visit `kde-mcp-marker-...` as a URL.

## Known Brave quirks worth flagging
- Brave may need `--enable-features=AccessibilityRemoteUIController` to expose URL bar via AT-SPI on some builds. If the marker is consistently absent, that's the root cause; document in S6-result.md and add to bug-bounty for v2.
````

- [ ] **Step 2: Verify**

```bash
test -f docs/runbook/scenarios/S6-brave-form-fill.md
```

- [ ] **Step 3: Commit**

```bash
git add docs/runbook/scenarios/S6-brave-form-fill.md
git commit -m "docs(runbook): scenario S6 — Brave URL-bar form-fill"
```

---

## Task 12: Cross-reference and drift check

**Files:**
- No file changes if all checks pass.
- If checks find drift, modify the offending file.

- [ ] **Step 1: Verify wire-name parity**

The runbook should reference every wire name in `tests/integration/tools_list.rs::EXPECTED_WIRE_NAMES` and no extras.

```bash
expected="$(awk '/EXPECTED_WIRE_NAMES/,/];/' tests/integration/tools_list.rs | grep -oE '"[^"]+"' | tr -d '"' | sort -u)"
mentioned="$(grep -hoE 'mcp__kde-mcp__[a-z_.]+' docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/scenarios/*.md | sed 's|mcp__kde-mcp__||' | sort -u)"
diff <(echo "$expected") <(echo "$mentioned")
```

Expected: empty diff (all 16 expected, none extra). If a wire name is missing → add it to Layer 2 with the standard probe (find a sensible no-op call from `src/server.rs`'s `#[tool(...)]` block).

- [ ] **Step 2: Verify all step IDs are unique and sequential**

```bash
grep -hoE '^### [LS][0-9]+\.[A-Za-z0-9]+' docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/scenarios/*.md | sort -V | uniq -d
```

Expected: empty output (no duplicates).

- [ ] **Step 3: Verify TC-Refs exist**

```bash
for ref in $(grep -hoE 'TESTING-CHECKLIST\.md §"[^"]+"' docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/scenarios/*.md | sort -u); do
    section="$(echo "$ref" | sed 's|.*§"\(.*\)"|\1|')"
    grep -qF "$section" docs/TESTING-CHECKLIST.md || echo "DANGLING TC-REF: $ref"
done
```

Expected: empty output (every referenced section exists in TESTING-CHECKLIST.md). If dangling → either update the TC-Ref to point at a real section or rename the section in TESTING-CHECKLIST.md.

- [ ] **Step 4: Verify KNOWN-FIXES.md is reachable from all scenario files**

```bash
for f in docs/runbook/scenarios/*.md; do
    grep -qE '(F[0-9]+|KNOWN-FIXES)' "$f" || echo "MISSING FIX-REF: $f"
done
```

Expected: empty output (every scenario references at least one F-code from KNOWN-FIXES.md).

- [ ] **Step 5: Verify the runbook builds a coherent dependency graph**

```bash
grep -hE '^\*\*Depends-on:' docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/scenarios/*.md | grep -oE '[LS][0-9]+\.[A-Za-z0-9]+' | sort -u > /tmp/deps.txt
grep -hoE '^### [LS][0-9]+\.[A-Za-z0-9]+' docs/runbook/AUTONOMOUS-VERIFICATION.md docs/runbook/scenarios/*.md | sed 's|^### ||' | sort -u > /tmp/defined.txt
comm -23 /tmp/deps.txt /tmp/defined.txt
```

Expected: empty (every depended-on ID is defined somewhere). If non-empty → fix the typo or define the missing step.

- [ ] **Step 6: If any check failed, fix inline and commit**

```bash
# Only if changes:
git add docs/runbook/
git commit -m "docs(runbook): fix cross-references found by drift check"
```

If everything was clean, this task is a no-op (no commit).

---

## Task 13: Wire the runbook into existing docs

**Files:**
- Modify: `README.md` — add a one-paragraph pointer.
- Modify: `docs/TESTING-CHECKLIST.md` — add a header note linking to the runbook.

- [ ] **Step 1: Update the README**

Find the README's "getting started" / status section and, after the existing code block, insert the pointer below:

```markdown
**Autonomous verification:** A fresh agent session can run end-to-end tests by following `docs/runbook/AUTONOMOUS-VERIFICATION.md`. The runbook covers all 16 tools, six product scenarios (S1–S6), and 4 of 6 bug-bounty probes. Per-run artefacts land in `docs/runbook/runs/<id>/` (gitignored). Auto-fix is mechanical only via `docs/runbook/KNOWN-FIXES.md` — no AI debug loop. **Prerequisite:** `./packaging/install.sh` must have been run at least once.
```

- [ ] **Step 2: Update TESTING-CHECKLIST.md**

At the very top of `docs/TESTING-CHECKLIST.md`, immediately after the title line, insert:

```markdown
> **Some of this is automated.** The runbook at `docs/runbook/AUTONOMOUS-VERIFICATION.md` covers the deterministic items; what remains here is for human eyes (color fidelity, typing feel, manual UX gut-checks) or items that need EIS-frame instrumentation. A fresh session running the runbook will create `docs/runbook/runs/<id>/summary.md` listing which checklist items it touched.
```

- [ ] **Step 3: Verify edits**

```bash
grep -F 'Autonomous verification' README.md
grep -F 'Some of this is automated' docs/TESTING-CHECKLIST.md
```

Both should produce one match each.

- [ ] **Step 4: Smoke-read the runbook end-to-end**

Manual: open `docs/runbook/AUTONOMOUS-VERIFICATION.md` and skim it as if you'd never seen it. Acceptance: every step has the required template fields; nothing references a step that doesn't exist; bash commands are sensible; the resume protocol is unambiguous.

If anything reads awkwardly → fix inline. Tiny doc-only fixes don't need their own task.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/TESTING-CHECKLIST.md
# plus any smoke-read fixes:
git add docs/runbook/ 2>/dev/null || true
git commit -m "docs: link autonomous verification runbook from README + checklist"
```

---

## Final verification

After all 13 tasks land:

```bash
# 1. Repo is clean.
git status
# expect: working tree clean

# 2. Eight new files exist + .gitignore + 2 doc updates.
git log --oneline -15
# expect: 13 commits prefixed docs(runbook) or docs:

# 3. Runbook directory is structurally correct.
tree docs/runbook -L 2
# expect:
# docs/runbook/
# ├── AUTONOMOUS-VERIFICATION.md
# ├── KNOWN-FIXES.md
# ├── scenarios/
# │   ├── S1-situational-awareness.md
# │   ├── S2-drive-kcalc.md
# │   ├── S3-ocr-fallback.md
# │   ├── S4-clipboard-transport.md
# │   ├── S5-window-management.md
# │   └── S6-brave-form-fill.md
# └── (no runs/ dir yet — agent creates it on first execution)

# 4. Hermetic tests still pass (we changed no Rust code).
cargo test --workspace 2>&1 | grep '^test result'
# expect: 4 lines, each `ok. .* 0 failed`

# 5. fmt and clippy still clean.
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
# expect: no output, exit 0
```

If all 5 final checks pass, the plan is complete. The runbook is ready for a fresh agent session to execute.

---

## What this plan does NOT do

- Doesn't execute the runbook. That's a separate exercise — open a fresh agent session, point it at the runbook, let it run.
- Doesn't add Phase 5 (virtual mode) verification. Explicit non-goal per spec.
- Doesn't add the EIS-frame instrumentation needed for B1 / B2 — deferred to v2.
- Doesn't modify any Rust code. Pure docs.
- Doesn't auto-trigger on git commits / CI. The runbook is manually invoked.
