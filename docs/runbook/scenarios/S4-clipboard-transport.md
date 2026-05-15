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
