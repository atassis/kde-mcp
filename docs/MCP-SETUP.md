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

## Post-deploy smoke

Run after any `./packaging/install.sh` or daemon binary swap. Three tiers, fastest first. Stop at the tier that answers your question.

> `claude mcp list` reports `✗ Failed to connect` for HTTP entries regardless of actual state — its health probe is cosmetic, not a real TCP dial. Ignore it. Use the tiers below.

### Tier 1 — daemon wire shape (no Claude Code session, ~10 s)

```sh
# 1. Daemon process alive?
systemctl --user is-active kde-mcp-http.service     # expect: active

# 2. healthz returns?
curl -fsS http://127.0.0.1:7575/healthz | python3 -m json.tool
# expect: {"ok": true, "sessions": <N>, "uptime_s": <N>}

# 3. /mcp accepts unauthenticated initialize (per ADR-0015)?
curl -sS -X POST http://127.0.0.1:7575/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  | grep -c '"name":"kde-mcp"'
# expect: 1

# 4. OAuth endpoints stay gone (ADR-0015 invariant)?
for p in .well-known/oauth-protected-resource .well-known/oauth-authorization-server oauth/token; do
  printf '%-50s → %s\n' "/$p" "$(curl -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:7575/$p)"
done
# expect: all 404
```

If any of these fail, the rest of the tiers won't pass — fix the daemon first. Check `journalctl --user -u kde-mcp-http.service -n 20`.

### Tier 2 — Claude Code can call a tool (~1 min)

Pick a session to test in:

- **Current session** (this one, if MCP tools are loaded): ask the model to call `mcp__kde-mcp__observe` with `{ "detail": "summary" }`. Tool is in the read-only allowlist, so it runs without a permission prompt. Expect a structured response with `windows`, `screens`, `cursor`.
- **Fresh session** (when you want a clean-slate test, e.g. right after redeploying): in a separate terminal, run `claude` in this repo, then ask "call mcp__kde-mcp__observe with detail summary."

If the model says it doesn't see the tool, run `/mcp` in that session to force a reconnect. If still missing, the in-process needs-auth cache may be stuck — exit and restart that Claude Code instance.

Diagnostic of last resort: read `~/.cache/claude-cli-nodejs/<repo>/mcp-logs-kde-mcp/<newest>.jsonl`. A healthy connect looks like `"Successfully connected (transport: http) in <NN>ms"`. Anything else with `"Skipping connection (cached needs-auth)"` or `"401"` means a stale auth attempt is cached.

### Tier 2.5 — One real-use-case scenario (~30 s – 3 min, fresh session preferred)

Between "tool returns the right shape" and "everything is fine" sits "the user-facing scenario actually completes." Six scenarios live under `docs/runbook/scenarios/`:

| ID | Use case | Pre-req | Approx cost |
|---|---|---|---|
| **S1** | Situational awareness — `observe` returns coherent snapshot | ≥1 window open | ~30 s, read-only |
| **S2** | Drive KCalc end-to-end (open → 47×23 → return result) | `kcalc` installed | ~2 min, destructive (fires policy gate) |
| **S3** | OCR fallback for canvas/PDF/image content | `magick`, `gwenview`, `tesseract` | ~3 min |
| **S4** | Clipboard as transport with prior-content restore | `wl-clipboard` | ~1 min, destructive |
| **S5** | Window management (move window to right half) | ≥1 user-focusable window | ~1 min, destructive |
| **S6** | Brave form-fill (type into URL bar) | Brave + ≥1 tab | ~1 min, destructive |

**Pick by what you changed:**

- Transport / auth / daemon wiring → **S1** is the minimal post-deploy gate.
- Keyboard / clipboard → **S4** + **S6**.
- Window control → **S5**.
- Policy gate / destructive paths → **S2** (exercises notification approvals).

In a fresh `claude` session in this repo, ask: *"Read `docs/runbook/scenarios/S1-situational-awareness.md` and execute it."* The model reads the file's Steps/Accept/On-fail and produces a PASS/FAIL with the parsed fields.

### Tier 3 — autonomous end-to-end (10+ min, fresh session only)

For phase-boundary verification, not per-deploy. Start a fresh `claude` session in this repo and ask it to follow `docs/runbook/AUTONOMOUS-VERIFICATION.md` Layers 1–3 (Layer 4 is bug-bounty, optional). The runbook will create `docs/runbook/runs/<id>/summary.md` listing pass/fail per step + per scenario. Heavyweight; do not run on every deploy.

## Quick sanity from any state

```sh
./packaging/install.sh         # rebuild + redeploy + flush stale claude.json state
# then run Tier 1 above
```

The `install.sh` reruns `claude mcp remove kde-mcp` + re-add, which clears any leftover auth header / discovery cache from older daemons.
