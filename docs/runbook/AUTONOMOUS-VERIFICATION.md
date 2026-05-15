# Autonomous verification runbook — kde-mcp

A fresh Claude Code session should be able to verify kde-mcp end-to-end by reading this file top-to-bottom and executing each step. Auto-fix is mechanical only — see `KNOWN-FIXES.md`. There is no AI debug loop; novel failures get logged and the run continues.

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
