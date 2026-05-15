# MCP setup — how kde-mcp is wired into Claude Code

Records the install layout chosen on **2026-05-15**, the choices behind it, and the rebuild workflow. Replaces the earlier "binary lives at `target/release/kde-mcp`" approach.

---

## Install layout

| Artifact | Path | Owner |
|---|---|---|
| Binary | `~/.local/bin/kde-mcp` | This file (rebuild = `cargo install --force`) |
| `.desktop` | `~/.local/share/applications/kde-mcp.desktop` | Points `TryExec`/`Exec` at the installed binary |
| MCP entry | `~/.claude.json` → `mcpServers.kde-mcp` | User-scope; created via `claude mcp add` |
| Tool allowlist | `~/.claude/settings.json` → `permissions.allow` | 8 read-only tools pre-approved |
| Policy file | `~/.config/kde-mcp/policy.toml` *(not yet created)* | Defaults to `full` tier when missing |
| Approvals state | `~/.local/state/kde-mcp/approvals.json` | Persistent Session approvals; gate manages it |
| Audit log | `~/.local/state/kde-mcp/audit.jsonl` | One JSONL record per `tools/call` |
| Per-mode lockfile | `$XDG_RUNTIME_DIR/kde-mcp/live.lock` | Acquired at startup; one process per mode |

The binary at `~/.local/bin/kde-mcp` is a **copy**, not a symlink. Repo `target/release/kde-mcp` is dev-only after this point.

## Design choices

### Why user scope (`--scope user`), not project / local

User scope makes kde-mcp available from every Claude Code session on this machine. The alternative (project / local scope) would only expose it inside this repo, which defeats the point — the whole reason to have a desktop-control MCP is to use it from any working directory.

Tradeoff accepted: **every Claude Code session start fires a "Claude acquired your KDE desktop" notification** (per `src/notify.rs`, by design since Phase 0). If that becomes noisy, the fix is to make the acquire notification fire on first destructive call rather than at server boot — out of scope today.

### Why `~/.local/bin/`, not `target/release/`

XDG-conventional user install path. Decouples the runtime binary from the repo: rebuilds don't auto-propagate (you must re-run `cargo install`), but the installed copy doesn't break if you `cargo clean` or rebase the repo into a different state. Same model as any system package.

The earlier setup pointed `.desktop` at `target/release/kde-mcp`. That was dev-only; this file replaces it.

### Why only `--mode=live` registered, not virtual

Virtual mode (`kwin_wayland --virtual` + wayvnc bootstrap) is **code-complete but never live-verified** — Phase 5 in `docs/TESTING-CHECKLIST.md` is fully unchecked. Registering it as if it were ready would mislead. Once Phase 5 is verified, add it as `kde-mcp-virtual`:

```sh
claude mcp add --transport stdio --scope user kde-mcp-virtual \
  -- ~/.local/bin/kde-mcp --mode=virtual --vnc=5910
```

### Why allowlist 8 read-only tools, not all

The 8 destructive tools (`keyboard.type`, `keyboard.key`, `mouse`, `clipboard_write`, `act_on_element`, `bash`, `text_editor.edit`, `window_control`) should always prompt — defense-in-depth alongside kde-mcp's own per-app policy gate. The 8 read-only ones (`observe`, `inspect`, `screenshot`, `read_text`, `wait`, `window_query`, `clipboard_read`, `text_editor.view`) have no side effects worth a prompt: skipping the prompt is pure UX win.

Note: `screenshot` is marked read-only but its output captures pixels of whatever's on-screen, including potentially-sensitive content. Terminal-masking (`src/util/image.rs::TERMINAL_CLASSES`) mitigates the worst case. If you'd rather it prompted, remove `mcp__kde-mcp__screenshot` from the allowlist.

## Rebuild workflow

After any code change you want reflected in the running MCP server:

```sh
cd ~/repositories/ns/atassis/kde-mcp
cargo install --path . --root ~/.local --force
```

`--force` is required because `cargo install` errors when the binary already exists. The build runs in release mode by default. After install:

- New Claude Code sessions pick up the new binary at session start.
- An already-running Claude Code session keeps using the old binary in memory until restart.

If the binary path ever moves (e.g. you switch to `/usr/local/bin/`), re-template `~/.local/share/applications/kde-mcp.desktop` (`TryExec` + `Exec`) and run `kbuildsycoca6 --noincremental` so KApplicationTrader rediscovers the path — otherwise `org.kde.KWin.ScreenShot2` calls will be denied at the bus.

## Operational gotchas

1. **One-session-at-a-time.** The live-mode lockfile (`$XDG_RUNTIME_DIR/kde-mcp/live.lock`) means only one Claude Code session can hold kde-mcp at any moment. A second concurrent session will see the server fail to start with `lock_unavailable, holder_pid=...`. Tools from that second session will be unavailable until the first releases. Not a bug — by ADR-0003 (mode-per-process).

2. **First destructive call against any app triggers a KDE notification.** kde-mcp's policy gate prompts via `org.freedesktop.Notifications` with Allow once / Allow this session / Deny. "Allow this session" persists to `~/.local/state/kde-mcp/approvals.json`. Until the user clicks, the gate returns `app_approval_required`.

3. **No `policy.toml` yet** = every app defaults to `full` tier. Per-app approval still gates destructive calls (point 2 above), so this isn't a security gap — just a heads-up that tier-based restrictions (`view_only`, `click_only`) aren't in effect until you write the file.

4. **Audit log grows unbounded** until Phase 6 rotation is verified (`docs/TESTING-CHECKLIST.md` line 128). If you batch heavy use, `tail` it and rotate manually for now.

## Uninstall

```sh
claude mcp remove --scope user kde-mcp
rm ~/.local/bin/kde-mcp
rm ~/.local/share/applications/kde-mcp.desktop
kbuildsycoca6 --noincremental
# optional — also clears persistent approvals + audit history:
rm -rf ~/.local/state/kde-mcp/
# leave ~/.claude/settings.json's permissions.allow entries; they're inert without the server.
```

## Verification

```sh
claude mcp list
# expect: kde-mcp: ~/.local/bin/kde-mcp --mode=live - ✓ Connected

# In a Claude session:
# > mcp__kde-mcp__observe with detail=summary
# should return the windows list without prompting (allowlisted).
```
