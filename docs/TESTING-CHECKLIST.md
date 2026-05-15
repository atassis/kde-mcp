# Testing & verification checklist — to run in parallel / after autonomous writing

> **Some of this is automated.** The runbook at `docs/runbook/AUTONOMOUS-VERIFICATION.md` covers the deterministic items; what remains here is for human eyes (color fidelity, typing feel, manual UX gut-checks) or items that need EIS-frame instrumentation. A fresh session running the runbook will create `docs/runbook/runs/<id>/summary.md` listing which checklist items it touched.

The autonomous push through Phases 3–6 wrote a lot of code that has unit tests but hasn't been run live. This document is the operator's checklist for actually exercising it on the dev machine. Items are grouped by phase and ordered roughly from "lightest live impact" to "heaviest."

**Convention:** each `--mode=live` invocation fires the "Claude is using your computer" KDE notification. Group fixtures into batches if you don't want a dozen notifications across the morning.

---

## Phase 2 live smoke (deferred during autonomous work)

These were code-complete at the start of this round but the live runs weren't done.

- [ ] **D.1** `keyboard.key ctrl+Tab` against a focused browser → tab switches.
- [ ] **D.2** `clipboard_write` + `clipboard_read` round-trip → content matches.
- [ ] **D.3** `mouse click` at `(400, 20)` against focused Brave → some tab gets clicked.
- [ ] **D.4** `keyboard.type "hello"` against a focused text field → "hello" appears, `used_clipboard=false`.
- [ ] **D.5** `keyboard.type "Привет"` against a focused text field → Cyrillic appears, `used_clipboard=true`, prior clipboard restored.
- [ ] Verify no clipboard leak: after D.5, manually `wl-paste` and confirm it's the pre-test content.

Procedure for each: `tests/integration/SMOKE.md` §D.

If all green: `git tag phase-2`.

---

## Phase 3 — policy gate

- [ ] **Policy file loads at startup.** `~/.config/kde-mcp/policy.toml` is read, parsing errors surfaced as `permission_denied` on first destructive tool call. Test by writing a malformed `policy.toml` and observing the response.
- [ ] **Default tier is `full`** when an app isn't in the policy table. Test by calling `mouse.click` against a focused unknown-class app (e.g. `kate`).
- [ ] **`view_only` tier** rejects every destructive tool. Test by adding `[apps."brave-browser"] tier = "view_only"` to `policy.toml` and calling `keyboard.type` against focused Brave → expect `tier_restricted`.
- [ ] **`click_only` tier** rejects `keyboard.type` but allows `mouse.click`. Test against focused Konsole with `tier = "click_only"`.
- [ ] **`click_only` keyboard.key allowlist** — `Ctrl+C`, `Tab`, arrow keys allowed; arbitrary `Ctrl+Shift+T` rejected.
- [ ] **Per-app approval flow:**
  - [ ] First destructive call against an unapproved app returns `app_approval_required` with `notification_id`.
  - [ ] The notification appears in KDE notification UI with "Allow once / Allow this session / Deny" action buttons.
  - [ ] "Allow once" → next call against same app succeeds, but the call after that re-prompts.
  - [ ] "Allow this session" → all subsequent calls against same app succeed without re-prompting.
  - [ ] "Deny" → returns `permission_denied`; subsequent calls re-prompt.
  - [ ] Concurrent retry during pending approval returns `app_approval_required, still_pending: true`.
- [ ] **Sentinel warnings:**
  - [ ] Destructive tool against `konsole` returns `warnings: ["target_app:konsole — equivalent to shell access"]`.
  - [ ] Destructive tool against `dolphin` returns the `filesystem_access` warning.
  - [ ] Destructive tool against `systemsettings` returns the `system_settings` warning.
- [ ] **Meta+Esc cancel shortcut:**
  - [ ] After process startup, pressing Meta+Esc anywhere fires the KDE "Claude aborted by user" notification.
  - [ ] An in-flight `keyboard.type` of a long string aborts mid-emission and returns `cancelled, cancelled_at_stage: "keyboard.type/char-N"`.
  - [ ] Subsequent tool calls work normally (token resets per call).
  - [ ] Cancel works during `mouse drag` (aborts after the current frame).
  - [ ] Cancel works during long `bash` invocations (signals SIGINT to the child).
- [ ] **Mode lock check** — Phase 3 makes the lock-held assertion explicit at gate entry. Manually delete `$XDG_RUNTIME_DIR/kde-mcp/live.lock` while a tool call is mid-flight (race; may be hard to hit). Should return `lock_unavailable`.

---

## Phase 4 — remaining destructive tools

### bash
- [ ] **First call lazily spawns a PTY.** Subsequent calls reuse the same PTY (persistent session — `cd /tmp` then `pwd` returns `/tmp`).
- [ ] **Output truncation at 16KB** with `truncated: true` + `full_output: "file:///tmp/.../bash-out-<id>.txt"` resource_link.
- [ ] **Blocklist** rejects each of:
  - [ ] `rm -rf /` (and variants ending at root)
  - [ ] `rm -rf ~` / `rm -rf $HOME`
  - [ ] `dd of=/dev/sda` / `dd of=/dev/sdb1`
  - [ ] `mkfs.ext4 /dev/sda1`
  - [ ] `:(){:|:&};:` fork bomb
  - [ ] Plain `sudo …` (unless `--allow-sudo` flag passed at server startup)
  - [ ] Plain `pkexec …`
- [ ] **Timeout handling** — `command: "sleep 200", timeout_ms: 500` returns within ~600ms with `timeout` error.
- [ ] **restart=true** tears down the existing PTY; the next call sees a fresh environment (no preserved `cd`).
- [ ] **Sensitive-path warnings** — `bash` reading `~/.ssh/id_rsa` adds `warnings: [...]` to the response.

### text_editor.edit
- [ ] `command: "create"` makes a new file with `file_text`; idempotent on identical re-create (no error, no-op).
- [ ] `command: "str_replace"` succeeds when `old_str` matches exactly once.
- [ ] `command: "str_replace"` fails with `target_not_found, closest_match: "<line>"` when `old_str` is missing.
- [ ] `command: "str_replace"` fails with `target_not_found, multiple_matches: N` when `old_str` matches twice.
- [ ] `command: "insert"` with `insert_line: 0` prepends; `insert_line: N` inserts after line N (1-indexed inclusive).
- [ ] **Sensitive-path warnings** — editing `~/.ssh/config` adds the warning; `/etc/foo`, `~/.aws/credentials`, etc. likewise.
- [ ] **Response snippet** — returned content is ±4 lines of context around the edit per spec.

### wait
- [ ] `condition: "seconds", seconds: 1.5` blocks ~1.5s then returns `matched: true, elapsed_ms: ~1500`.
- [ ] `condition: "window_appears", match: {class: "kcalc"}` — open `kcalc` within 5s, tool returns `matched: true, matched_window: {ref, class}`.
- [ ] `condition: "window_disappears"` — close a window, returns `matched: true`.
- [ ] `condition: "element_appears", element_match: {role: "PushButton", name_pattern: "OK"}` — opens a dialog with an OK button, returns when it appears.
- [ ] `condition: "screen_change", pixel_threshold: 0.05` — diff frames against initial; returns when pixel-diff exceeds threshold.
- [ ] **Timeout** — `timeout_ms: 500` against a condition that never matches returns `timeout` with `closest_match` and `current_windows`.

### read_text (OCR)
- [ ] **Tesseract installed via the install script.** `which tesseract` returns a path.
- [ ] `read_text region: "window:<ref>"` against a focused PDF viewer returns recognizable text with `trust: "external"`.
- [ ] **Multi-language** — `lang: "rus"` against a Russian-language PDF returns Cyrillic text.
- [ ] `include_bboxes: true` returns per-word bboxes + confidences.
- [ ] **External command failure** — uninstall tesseract → tool returns `external_command_failed`.

### window_control
- [ ] `action: "focus", window_uuid: <ref>` brings the window to front.
- [ ] `action: "minimize"` / `"maximize"` / `"restore"` work as expected.
- [ ] `action: "move", geometry: [100, 100, 800, 600]` repositions/resizes.
- [ ] `action: "close"` closes the window.
- [ ] `action: "set_fullscreen", value: true` toggles fullscreen.
- [ ] `action: "set_above", value: true` sets always-on-top.

### Phase 1/2 deferrals lifted in Phase 4
- [ ] `inspect target=focused` — after a focus tracker is running, returns the currently-focused element's subtree.
- [ ] `observe.focused_element` field is populated (not null) after first user focus event.
- [ ] `inspect target=window:<ref>` — resolves window UUID → AT-SPI app via pid mapping, returns app's accessible subtree.

---

## Phase 5 — virtual mode

The user-facing claim: `kde-mcp --mode=virtual --vnc=5910` spawns an isolated KDE Plasma session in a child process, registers as MCP, and routes all tools to that child session instead of the user's real one. `wayvnc` exposes it on port 5910 for visual inspection.

- [ ] **Bootstrap succeeds** — `kde-mcp --mode=virtual --vnc=5910` outputs "ready (mode=virtual)" within ~5 seconds, no errors.
- [ ] **Child processes alive** — `pgrep -af 'kwin_wayland --virtual'` shows one process; `pgrep -af wayvnc` shows one.
- [ ] **VNC connection** — `vncviewer localhost:5910` opens a black KDE desktop with no apps initially.
- [ ] **`bash launch=kcalc`** opens kcalc inside the virtual session; visible via VNC.
- [ ] **Tool calls route to virtual** — `observe` in virtual mode returns the virtual session's windows, NOT the live session's.
- [ ] **Concurrent live + virtual** — start `kde-mcp --mode=live` AND `kde-mcp --mode=virtual` simultaneously. Both run, each has its own lockfile.
- [ ] **Teardown** — Ctrl-C the virtual binary → all child processes (kwin_wayland, wayvnc, at-spi-bus-launcher) terminate. No zombies.
- [ ] **AT-SPI bootstrap** — `inspect target=window:<ref>` works inside virtual session (validates distro-specific a11y bus launch).
- [ ] **Audit log** — virtual mode writes to its own `~/.local/state/kde-mcp/audit-virtual.jsonl` (or shared with mode field).

---

## Phase 6 polish

- [ ] **Audit log rotates at 50MB** — write 51MB of garbage to the audit log, next call rotates to `.1`, keeps last 5.
- [ ] **xkbcommon keymap-aware typing** — change system keymap to dvorak; `keyboard.type "asdf"` types the correct characters per dvorak layout (not qwerty).
- [ ] **Non-US character support** — type `'é'` directly without falling back to clipboard-paste.
- [ ] **Real SHA-256 in audit redactor** — `clipboard_write {content: "secret"}` audit record has `args.content.sha256` matching `sha256("secret")` (not the placeholder hash).
- [ ] **Sensitive-path coverage audit** — `text_editor.edit` warnings on every path in `~/.ssh/`, `~/.gnupg/`, `/etc/`, `~/.config/keepassxc/`, `~/.config/git/`, `~/.aws/`, `~/.kube/`.
- [ ] **mouse hold / down / up / hold_key** — implemented (Phase 6 deferred from Phase 2).
- [ ] **mouse modifier+click** — implemented via combined keyboard+pointer EIS session.
- [ ] **Long-running EIS connection** option (env var or config) replaces per-call handshake; measure latency savings.

---

## Cross-cutting / non-functional

- [ ] **Audit log contains every destructive tool call** with wire name. Tail-f during a 5-minute session and confirm.
- [ ] **Trust-wrapper coverage** — grep every response struct's `Tagged<String>` fields; manually verify each is `external` when the source is external, `trusted` when it's from an OS API.
- [ ] **Schema sanity** — `tools/list` over the wire returns 16 tools, every input schema has `required` array where applicable, all `annotations` are correctly set.
- [ ] **Concurrent calls** — fire two tool calls back-to-back via the JSON-RPC wire. Both should serialize cleanly; second tool's audit record arrives after the first.
- [ ] **Bus disconnect recovery** — restart `kwin_wayland` while server is running. Next tool call returns `bus_disconnected`. Server stays alive; subsequent calls work after KWin reconnects.
- [ ] **`.mcpb` extension bundle** — `cargo install --path . --root ~/.local`, then `claude plugin install ~/.local/bin/kde-mcp` (when this packaging exists).
- [ ] **Memory leak check** — run the audit-stress fixture (1000 calls in a loop) and observe `RES` doesn't grow unbounded.

---

## CI / future-proofing

- [ ] GitHub Actions workflow (when there's a remote) that runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` on every push.
- [ ] **rmcp version bump** smoke — when rmcp publishes 1.8, bump the pin in `Cargo.toml`, re-run the smoke. Expect no behavioural change.
- [ ] **atspi version bump** smoke — atspi is pre-1.0; next bump may need ref-encoding adjustments.
- [ ] **KDE Plasma 6.x → 7.x** — when Plasma 7 ships, re-verify `workspace.activeWindow`, `windowList()`, `cursorPos`, `activeScreen` Q_PROPERTY names. If they're renamed (as Plasma 5→6 was), update the JS templates in `src/tools/window.rs` and `src/tools/observe.rs`.

---

## Manual UX gut-check

These are subjective and need human eyes.

- [ ] **`screenshot` color fidelity** — manually inspect the PNG. Compare to a real spectacle screenshot of the same screen. Look for: color casts, gamma differences, premultiplied-alpha banding on translucent app windows.
- [ ] **Cursor visibility** — when `include_cursor: true`, the cursor is drawn in the PNG at the correct position.
- [ ] **Fractional scaling** — on a mixed 100% + 150% display setup, screenshot of each display has the right pixel dimensions; window geometries match the actual visible rectangles.
- [ ] **Typing speed** — `keyboard.type "the quick brown fox"` (19 chars) should complete in under 500ms. If it's > 1s the inter-keystroke delays are too generous.
- [ ] **Audit log readability** — open `~/.local/state/kde-mcp/audit.jsonl` in your editor of choice. Each line is human-readable. Find anything that's mis-formatted.

---

## Quick-fire bug-bounty list (things that might be subtly wrong)

- [ ] `keyboard.type` doesn't release shift between consecutive capital letters — typing `"AB"` may emit `Shift↓ A↓ A↑ B↓ B↑ Shift↑` (current code releases shift between chars, then re-presses). Verify the optimization-vs-correctness tradeoff is OK.
- [ ] `mouse click` at the same coordinate twice in quick succession might be interpreted as double-click by Qt's `qApp->doubleClickInterval()`. Verify single-click semantics with two separate `mouse click` calls 200ms apart.
- [ ] `clipboard_write` for binary content (`encoding: "base64"`) — does the base64 decode happen server-side before piping to `wl-copy`? Verify the round-trip.
- [ ] `screenshot region=[x,y,w,h]` with a rect that crosses multi-monitor boundaries — what's returned?
- [ ] `inspect` over a tree with circular parent_id (shouldn't happen, but defensively) — does the walker infinite-loop or hit max_elements?
- [ ] `wait condition=screen_change` on a static screen — does it timeout cleanly?

---

## How to find this list when needed

Save the link to this file. Future-you running cold won't remember which Phase needs which test until reading the spec — this checklist is the shortcut.
