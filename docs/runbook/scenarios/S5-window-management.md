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
