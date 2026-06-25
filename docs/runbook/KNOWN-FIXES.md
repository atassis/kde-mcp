# Known fixes — signature → mechanical command

The runbook's executing agent consults this table when a step fails. **Apply at most one fix per failure**, retry the step **at most once**, then move on regardless of the retry's outcome. No fuzzy matching, no AI judgement — exact regex match against the failure's error code or stderr.

Matching priority: top-to-bottom; first match wins.

| # | Match (regex against `error.code` ∪ `stderr` ∪ `error.details.detail`) | Fix | Retry step once? | Notes |
|---|---|---|---|---|
| F1 | `binary not found at .*kde-mcp` OR `No such file or directory: .*\.local/bin/kde-mcp` | `./packaging/install.sh --skip-mcp` | yes | Rebuilds + re-installs the binary. Skips Claude config so we don't loop the install. |
| F2 | `not_authorized.*ScreenShot2` | `kbuildsycoca6 --noincremental` | yes | KApplicationTrader cache stale. Rebuild it; KWin will re-authorize on next call. |
| F3 | `busy` | (none) | no | Another Claude Code session holds the mutation lease. **Log and retry the dependent step once** — the leaseholder's burst is usually short. Subsequent persistent `busy` is logged + the step is marked failed. |
| F4 | `wl-paste: command not found` OR `wl-copy: command not found` | (none) | no | wl-clipboard not installed. Skip all S4 (clipboard transport) steps + L2 clipboard_read/clipboard_write. Log dep-missing. |
| F5 | `tesseract.*not found` OR `external_command_failed.*tesseract` | (none) | no | OCR backend missing. Skip S3. |
| F6 | `external_command_failed.*magick` OR `magick: command not found` | (none) | no | ImageMagick missing. Cannot generate S3 fixture. Skip S3. |
| F7 | `kde-mcp .* (connection refused|broken pipe|server crashed)` | (none) | no | MCP server died. **Abort the run.** Tell the user to restart their Claude Code session. |
| F8 | `app_approval_required` AND `still_pending: true` | (wait 30 s, then no fix — assume user clicked) | yes | Policy-gate prompt is in flight. The executing agent should sleep 30 s for the user to click Allow, then retry exactly once. If still pending, log and move on. |
| F9 | `cargo test` returns non-zero from L1.2 with `expected 12 wire tool names` | (none) | no | Runbook is **outdated** vs the binary's tool set. Abort the run and tell the user the runbook needs updating. |

**Anything not matching:** log the full failure response to `runs/<id>/<step-id>-failure.md`, mark the step `failed`, continue to the next independent step. The dependent steps will record `skipped(reason: dep <step-id> failed)`.
