# S2 — Drive KCalc end-to-end

> "Open KCalc, compute 47 × 23, return the result."

The classic GUI-automation demo. Exercises: host-shell launch + window_query poll + window_control focus + keyboard.type + inspect + window_control close. End-to-end product flow.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 2 live smoke"
**Goal:** Drive KCalc to compute 47*23, confirm display reads 1081.
**Pre:** `kcalc` installed. If not: skip with `skipped(precondition: kcalc not installed)`.
**Depends-on:** L2.16 (window_control focus), L2.12 (keyboard.type), L2.2 (inspect), L2.6 (window_query).

## Steps
1. Check `kcalc` is available via the host shell: `command -v kcalc`. If exit non-zero, skip.
2. Launch via the host shell: `kcalc &` (e.g. `setsid kcalc >/dev/null 2>&1 &`).
3. Poll for the window: call `mcp__kde-mcp__window_query` with `{"action":"list"}` once per 200 ms until a window with `class == "org.kde.kcalc"` appears or 5 s elapses; record its `window_uuid`. Treat 5 s without a match as a fail ("kcalc didn't appear"). Save the matching `window_query` response to `runs/$run_id/S2-windows.json`.
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
- `runs/$run_id/S2-windows.json`
- `runs/$run_id/S2-inspect.json`
- `runs/$run_id/S2-result.md` with: input → result, PASS/FAIL, the chosen display element's role+name+value.

## On fail
- `app_approval_required` for kcalc → F8 (sleep 30s for user click, retry once).
- kcalc window absent after the 5 s poll → log "kcalc didn't appear", mark failed, skip cleanup.
- No element with numeric value → log all inspected elements' values to `S2-result.md`, mark failed.
- Otherwise: log + continue.

## Cleanup
`mcp__kde-mcp__window_control` with `{"action":"close", "window_uuid":"<uuid>"}`. If KCalc prompts to save (it shouldn't for a fresh calc), press Escape via `keyboard.key`.
