# S3 — OCR fallback

> "Read text from a window where AT-SPI doesn't expose it (canvas / PDF / image)."

Generates a known-text image on first run, opens it in an image viewer, runs `read_text` over the window. Tests the vision-fallback path. Output is `trust: external` per spec §4.7.

**TC-Ref:** TESTING-CHECKLIST.md §"Phase 4 — remaining destructive tools" / read_text
**Goal:** OCR a fixture image and confirm the recognised text matches the embedded probe string.
**Pre:** `magick` (ImageMagick), `gwenview` (or any image viewer), `tesseract` with `tesseract-data-eng` installed.
**Depends-on:** L2.4 (read_text — at least sanity-passed), L2.6 (window_query).

## Steps
1. Check deps via the host shell: `command -v magick && command -v gwenview && command -v tesseract`. If any missing, skip.
2. Generate fixture (idempotent — only if missing) via the host shell: `mkdir -p ~/.cache/kde-mcp-runbook && [ -f ~/.cache/kde-mcp-runbook/ocr-fixture.png ] || magick -background white -fill black -size 600x150 -gravity center caption:"kde-mcp ocr probe ${run_id}" ~/.cache/kde-mcp-runbook/ocr-fixture.png`. (Replace `${run_id}` in the caption call with the actual run id string before running so the image content is deterministic per run.)
3. Open in gwenview via the host shell: `setsid gwenview ~/.cache/kde-mcp-runbook/ocr-fixture.png >/dev/null 2>&1 &`.
4. Poll for the window: call `mcp__kde-mcp__window_query` with `{"action":"list"}` once per 200 ms until a window with `class == "org.kde.gwenview"` appears or 5 s elapses; record its `window_uuid`. Treat 5 s without a match as a skip (gwenview failed to launch — not a kde-mcp bug).
5. Call `mcp__kde-mcp__read_text` with `{"region":"window:<uuid from poll>", "lang":"eng"}`.
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
