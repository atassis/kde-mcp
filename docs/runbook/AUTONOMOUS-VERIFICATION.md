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
**Goal:** Capture pixels from the active screen via KWin ScreenShot2. Verifies the MCP-image-content delivery path (commit `77cf284`): the PNG arrives as a native image-content block — not as a base64 string inside a JSON text block — so Claude Code routes it to the model's vision pathway.
**Pre:** none.
**Depends-on:** L1.4
**Steps:**
1. Call `mcp__kde-mcp__screenshot` with `{"region":"active_screen"}`.
2. Save the response envelope (without the binary image data) to `runs/$run_id/L2.3-screenshot.json` — record the `content` block types, their `mimeType` fields, and the parsed metadata JSON. Do **not** dump the base64 bytes into the artefact (would balloon the file 5–10× without adding signal).
**Accept:**
- `isError` is false (or absent).
- `content` array has exactly two blocks: one `{"type":"image","mimeType":"image/png","data":<base64>}` plus one `{"type":"text","text":<metadata-json>}`.
- The text block parses as JSON containing `original_size:[w,h]`, `returned_size:[w,h]`, and `scale_factor:<float>` with all values > 0.
**Artefacts:** `L2.3-screenshot.json` (envelope only, no base64).
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

<!-- L2.5 removed by ADR-0013: `wait` tool retired (sleep-only form duplicated the host's sleep + any observe call refreshes the lease). -->

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

<!-- L2.8 removed by ADR-0013: `text_editor.view` retired; the host's native `Read` covers it. -->
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

<!-- L2.15 removed by ADR-0013: `bash` tool retired; Claude Code's native bash covers shell access. -->

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

<!-- L2.17 removed by ADR-0013: `text_editor.edit` retired; Claude Code's native Edit/Write covers file mutation. -->

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

A subset of the six items at `docs/TESTING-CHECKLIST.md:172`. Only the ones observable from *outside* the binary make sense in this layer; ones that need EIS-frame instrumentation are explicit non-goals for v1 (would require build flag + log capture; tracked as a future addition).

- **B3 (KEEP):** `clipboard_write content=<base64-encoded random 32 bytes> encoding=base64`; `clipboard_read`; decode response; assert bytes round-trip.
- **B4 (CONDITIONAL on multi-monitor):** Detect `>1` monitor in `env.json`. If yes: `screenshot region=[x,y,w,h]` straddling the seam; record returned dims and visually-inspect that both monitor regions are populated (save PNG for human eyes). If single-monitor: skip with `skipped(precondition: single monitor)`.
- **B5 (KEEP, retargeted post-ADR-0013):** Open Dolphin via the runbook executor's own bash (not an MCP tool — `bash` retired); poll `window_query` for the window UUID; `inspect target=window:<uuid> max_elements=10000`; assert returns in <5 s with `truncated=false` *or* `truncated=true` (either is OK — the failure is hanging / OOM); record element_count.
<!-- B6 retired by ADR-0013 — it tested `wait condition=screen_change`, and the `wait` tool is gone. -->

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
1. **Runbook executor's bash** (not an MCP tool — `mcp__kde-mcp__bash` was retired by ADR-0013): `setsid dolphin >/dev/null 2>&1 &`.
2. Poll `mcp__kde-mcp__window_query` with `{"action":"list"}` once per 200 ms until a window with `class == "org.kde.dolphin"` appears or 5 s elapses; record the UUID. Treat 5 s without a match as **skip** (Dolphin failed to launch — not a kde-mcp bug).
3. Record start time. Call `mcp__kde-mcp__inspect` with `{"target": "window:<uuid>", "max_elements": 10000}`. Record end time.
4. Save response to `runs/$run_id/L4.B5-inspect.json`.
**Accept:** Call returns within 5 seconds. Response has an `elements` array (`truncated: true` OR `truncated: false` — both acceptable). No timeout / crash.
**Artefacts:** `L4.B5-inspect.json`, `L4.B5-timing.txt`.
**On fail:** log + continue.
**Cleanup:** `mcp__kde-mcp__window_control` `{"action":"close","window_uuid":"<uuid>"}`.

<!-- L4.B6 removed by ADR-0013: it tested `wait condition=screen_change`; the `wait` tool is gone. If a future ADR re-adds a KWin-signal-driven condition wait, write a new bug-bounty case against that tool then. -->

---

### Deferred to v2
- **B1** — shift release between capital letters in `keyboard.type`. Requires `RUST_LOG=trace` capture of EIS frames; not externally observable.
- **B2** — mouse double-click coalescing by `qApp->doubleClickInterval()`. Needs a target app's event log or a custom probe widget.

Both are documented in `docs/TESTING-CHECKLIST.md` and will be picked up when EIS-frame instrumentation lands.

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
