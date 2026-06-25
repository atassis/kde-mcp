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
4. Host-side `sleep 0.3` (let focus settle).
5. `mcp__kde-mcp__keyboard.key` with `{"chord":"ctrl+l"}` (focus address bar; Brave's URL bar shortcut).
6. Host-side `sleep 0.2`.
7. `mcp__kde-mcp__keyboard.type` with `{"text":"kde-mcp-marker-${run_id}"}`.
8. Host-side `sleep 0.3` (let AT-SPI settle).
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
