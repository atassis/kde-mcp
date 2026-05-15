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
