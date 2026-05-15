# MCP setup — how kde-mcp is wired into Claude Code

Records the install layout chosen on **2026-05-15**, the choices behind it, and the rebuild workflow. Replaces the earlier "binary lives at `target/release/kde-mcp`" approach.

---

## TL;DR

```sh
./packaging/install.sh               # build + install + register at user scope + allowlist read-only tools
./packaging/install.sh --with-virtual  # also register the virtual-mode entry
./packaging/uninstall.sh             # reverse (--purge to also drop ~/.local/state/kde-mcp)
```

The script is idempotent. After a rebuild, re-run `./packaging/install.sh` to push the new binary to `~/.local/bin/`. The rest of this doc explains what those scripts do and why.

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
./packaging/install.sh    # rebuilds + replaces ~/.local/bin/kde-mcp; idempotent
```

Or run cargo directly if you want to skip the Claude-config steps:

```sh
cargo install --path . --root ~/.local --force
```

`--force` is required because `cargo install` errors when the binary already exists. The build runs in release mode by default. After install:

- New Claude Code sessions pick up the new binary at session start.
- An already-running Claude Code session keeps using the old binary in memory until restart.

If the binary path ever moves (e.g. you switch to `/usr/local/bin/`), re-template `~/.local/share/applications/kde-mcp.desktop` (`TryExec` + `Exec`) and run `kbuildsycoca6 --noincremental` so KApplicationTrader rediscovers the path — otherwise `org.kde.KWin.ScreenShot2` calls will be denied at the bus.

## Operational gotchas

1. **Multi-session coexistence with workflow-level lease.** Multiple Claude Code sessions can run kde-mcp concurrently (per ADR-0011). Mutating tool calls serialize via `$XDG_RUNTIME_DIR/kde-mcp/lease.json`: the first mutation acquires the lease for ~10s of idle, and other sessions' mutations block (returning `busy { holder_pid, holder_intent, lease_expires_in_ms }` after `acquire_timeout`, default 60s). Observation tools never block. Idle sessions cost nothing.

2. **First destructive call against any app triggers a KDE notification.** kde-mcp's policy gate prompts via `org.freedesktop.Notifications` with Allow once / Allow this session / Deny. "Allow this session" persists to `~/.local/state/kde-mcp/approvals.json`. Until the user clicks, the gate returns `app_approval_required`.

3. **No `policy.toml` yet** = every app defaults to `full` tier. Per-app approval still gates destructive calls (point 2 above), so this isn't a security gap — just a heads-up that tier-based restrictions (`view_only`, `click_only`) aren't in effect until you write the file.

4. **Audit log grows unbounded** until Phase 6 rotation is verified (`docs/TESTING-CHECKLIST.md` line 128). If you batch heavy use, `tail` it and rotate manually for now.

## Uninstall

```sh
./packaging/uninstall.sh           # removes MCP entries, binary, .desktop, allowlist
./packaging/uninstall.sh --purge   # also clears ~/.local/state/kde-mcp/ (audit log + approvals)
```

Manual equivalent:

```sh
claude mcp remove --scope user kde-mcp
rm ~/.local/bin/kde-mcp
rm ~/.local/share/applications/kde-mcp.desktop
kbuildsycoca6 --noincremental
# remove the mcp__kde-mcp__* entries from ~/.claude/settings.json by hand
# optional — also clears persistent approvals + audit history:
rm -rf ~/.local/state/kde-mcp/
```

## Verification

```sh
claude mcp list
# expect: kde-mcp: ~/.local/bin/kde-mcp --mode=live - ✓ Connected

# In a Claude session:
# > mcp__kde-mcp__observe with detail=summary
# should return the windows list without prompting (allowlisted).
```
