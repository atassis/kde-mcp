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

Each step calls one tool via the live MCP session, validates the response structurally (not exact-match — KDE state drifts), saves the response payload to `runs/$run_id/L2.<n>-<tool>.json`. Read-only tools first (no policy-gate prompts), destructive last.

### L2.1 — observe(detail=summary)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
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
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Drill into the AT-SPI tree of the focused window and return a flattened element list.
**Pre:** ≥1 window focused on the desktop.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__inspect` with `{"target":"focused"}`.
2. Save the raw response to `runs/$run_id/L2.2-inspect.json`.
**Accept:** response has `elements` array (may be empty if focus tracker not running — still passes); no `error`.
**Artefacts:** `L2.2-inspect.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.3 — screenshot(region=active_screen)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Capture pixels from the active screen via KWin ScreenShot2.
**Pre:** none.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__screenshot` with `{"region":"active_screen"}`.
2. Save the raw response to `runs/$run_id/L2.3-screenshot.json`.
**Accept:** response has `image.png` (base64 or resource_link) and `dims:[w,h]` with both > 0.
**Artefacts:** `L2.3-screenshot.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.4 — read_text(region=active_screen)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools"
**Goal:** OCR the active screen via Tesseract when AT-SPI doesn't expose text.
**Pre:** `tesseract` installed (`which tesseract`).
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__read_text` with `{"region":"active_screen","lang":"eng"}`.
2. Save the raw response to `runs/$run_id/L2.4-read_text.json`.
**Accept:** response has `text` field (may be empty); no panic.
**Artefacts:** `L2.4-read_text.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.5 — wait(condition=seconds)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools"
**Goal:** Block until a structured time condition is met, replacing arbitrary sleeps.
**Pre:** none.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__wait` with `{"condition":"seconds","seconds":0.2}`.
2. Save the raw response to `runs/$run_id/L2.5-wait.json`.
**Accept:** `matched:true`; `elapsed_ms` in `[150, 400]`.
**Artefacts:** `L2.5-wait.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.6 — window_query(action=list)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** List all windows on the desktop with class, title, geometry, and screen.
**Pre:** ≥1 window on the desktop.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__window_query` with `{"action":"list"}`.
2. Save the raw response to `runs/$run_id/L2.6-window_query.json`.
**Accept:** response has `windows` array; length matches L2.1's count within ±1 (for race).
**Artefacts:** `L2.6-window_query.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.7 — clipboard_read
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Read the current clipboard contents via wl-paste.
**Pre:** `wl-clipboard` installed (`which wl-paste`).
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__clipboard_read` with `{}`.
2. Save the raw response to `runs/$run_id/L2.7-clipboard_read.json`.
**Accept:** response has `content` field (possibly empty); no `error` other than `target_not_found` (legitimate empty clipboard).
**Artefacts:** `L2.7-clipboard_read.json`.
**On fail:** signature match against KNOWN-FIXES.md (F4 if wl-clipboard missing, F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.8 — text_editor.view(path=/etc/hostname)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Read a file or directory listing using an absolute path.
**Pre:** `/etc/hostname` exists (standard on all Linux distros).
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__text_editor.view` with `{"path":"/etc/hostname"}`.
2. Save the raw response to `runs/$run_id/L2.8-text_editor.view.json`.
**Accept:** response content contains the output of bash `hostname`.
**Artefacts:** `L2.8-text_editor.view.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.9 — clipboard_write (probe string)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Set the clipboard contents to a known probe string via wl-copy.
**Pre:** `wl-clipboard` installed (`which wl-copy`).
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__clipboard_write` with `{"content":"kde-mcp-probe-${run_id}"}` (substitute the actual `run_id` value).
2. Save the raw response to `runs/$run_id/L2.9-clipboard_write.json`.
**Accept:** `ok:true`.
**Artefacts:** `L2.9-clipboard_write.json` (also record the exact probe string written so L2.10 can compare).
**On fail:** signature match against KNOWN-FIXES.md (F4 if wl-clipboard missing, F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** S4 will restore later; if S4 doesn't run, the user's clipboard remains overwritten.

### L2.10 — clipboard_read (verifies L2.9)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Confirm the clipboard round-trip by reading back the probe string written in L2.9.
**Pre:** `wl-clipboard` installed; L2.9 passed.
**Depends-on:** L1.4, L2.9
**Steps:**
1. Call `mcp__kde-mcp__clipboard_read` with `{}`.
2. Save the raw response to `runs/$run_id/L2.10-clipboard_read.json`.
**Accept:** content equals the probe string written in L2.9.
**Artefacts:** `L2.10-clipboard_read.json`.
**On fail:** signature match against KNOWN-FIXES.md (F4 if wl-clipboard missing, F7 if connection error). Else log + continue.
**Cleanup:** none.

### L2.11 — keyboard.key(chord=F12)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Press named keys or chords via libei — F12 is mostly inert and safe for probing.
**Pre:** ≥1 window focused.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__keyboard.key` with `{"chord":"F12"}`.
2. Save the raw response to `runs/$run_id/L2.11-keyboard.key.json`.
**Accept:** no `error`. (F12 is mostly inert.)
**Artefacts:** `L2.11-keyboard.key.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.12 — keyboard.type(text=kdemcp)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Type literal text into the focused window without modifier keys.
**Pre:** ≥1 text-accepting window focused.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__keyboard.type` with `{"text":"kdemcp"}`.
2. Save the raw response to `runs/$run_id/L2.12-keyboard.type.json`.
**Accept:** no `error`; `chars_emitted:6`.
**Artefacts:** `L2.12-keyboard.type.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.13 — mouse(action=move)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Move the mouse cursor to coordinates via libei — move only, no click.
**Pre:** none.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__mouse` with `{"action":"move","x":100,"y":100}`.
2. Save the raw response to `runs/$run_id/L2.13-mouse.json`.
**Accept:** no `error`. (Move only; no click.)
**Artefacts:** `L2.13-mouse.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.14 — act_on_element
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Invoke a named action on an accessibility element by ref — the primary AT-SPI-first action.
**Pre:** L2.2 must have returned at least one element with non-empty `actions`. If no actionable element was returned, skip with `skipped(reason: no actionable element)`.
**Depends-on:** L1.4, L2.2
**Steps:**
1. Call `mcp__kde-mcp__act_on_element` with the ref and action name of the first actionable element found in L2.2's response.
2. Save the raw response to `runs/$run_id/L2.14-act_on_element.json`.
**Accept:** no `error`.
**Artefacts:** `L2.14-act_on_element.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.15 — bash(command=echo hello)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools"
**Goal:** Run a shell command via a persistent bash PTY session.
**Pre:** none.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__bash` with `{"command":"echo hello","restart":false}`.
2. Save the raw response to `runs/$run_id/L2.15-bash.json`.
**Accept:** response `stdout` = `"hello\n"`; `exit_code:0`.
**Artefacts:** `L2.15-bash.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.16 — window_control(action=focus)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools"
**Goal:** Focus/minimise/maximise/close a window by ref — verifying the KWin script control roundtrip.
**Pre:** L2.6 must have returned at least one window.
**Depends-on:** L1.4, L2.6
**Steps:**
1. Call `mcp__kde-mcp__window_control` with `{"action":"focus","window_uuid":"<L2.6's first window's uuid>"}` (substitute the actual uuid).
2. Save the raw response to `runs/$run_id/L2.16-window_control.json`.
**Accept:** no `error`.
**Artefacts:** `L2.16-window_control.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** none.

### L2.17 — text_editor.edit(command=create)
**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools"
**Goal:** Edit a file via create/replace/insert/undo — verifying the file-write path end-to-end.
**Pre:** `/tmp` is writable.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__text_editor.edit` with `{"command":"create","path":"/tmp/kde-mcp-runbook-probe.txt","file_text":"probe"}`.
2. Save the raw response to `runs/$run_id/L2.17-text_editor.edit.json`.
3. Call `mcp__kde-mcp__bash` with `{"command":"rm /tmp/kde-mcp-runbook-probe.txt","restart":false}` to clean up the probe file.
**Accept:** `ok:true`.
**Artefacts:** `L2.17-text_editor.edit.json`.
**On fail:** signature match against KNOWN-FIXES.md (F7 if connection error, F8 if approval pending). Else log + continue.
**Cleanup:** `bash` `rm /tmp/kde-mcp-runbook-probe.txt` (already Step 3 above).

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
