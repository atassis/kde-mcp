#!/usr/bin/env bash
# Install kde-mcp into ~/.local/ and wire it into Claude Code at user scope.
#
# Idempotent — safe to re-run after rebuilds or config changes.
#
# Usage:
#   ./packaging/install.sh                       # auto-detect transport (daemon on KDE-Wayland; stdio else)
#   ./packaging/install.sh --transport=daemon    # force HTTP daemon (ADR-0012)
#   ./packaging/install.sh --transport=stdio     # force one-stdio-child-per-session
#   ./packaging/install.sh --with-virtual        # also register virtual-mode entry (stdio only)
#   ./packaging/install.sh --skip-mcp            # build/install binary only; no Claude config
#   ./packaging/install.sh --skip-allowlist      # register MCP but don't touch settings.json
#   ./packaging/install.sh --prefix=/usr/local   # custom install prefix (needs writable dest)
#
# See docs/MCP-SETUP.md and docs/adr/0012-singleton-daemon-http.md.

set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
TRANSPORT=""
WITH_VIRTUAL=0
SKIP_MCP=0
SKIP_ALLOWLIST=0

usage() {
    sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --with-virtual)    WITH_VIRTUAL=1;     shift ;;
        --skip-mcp)        SKIP_MCP=1;         shift ;;
        --skip-allowlist)  SKIP_ALLOWLIST=1;   shift ;;
        --prefix=*)        PREFIX="${1#*=}";   shift ;;
        --transport=*)     TRANSPORT="${1#*=}"; shift ;;
        -h|--help)         usage; exit 0 ;;
        *)                 echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

# Auto-detect transport if not specified. Unix-socket by default on
# KDE+Wayland; stdio elsewhere (headless server OS, GNOME, non-systemd).
# `http` is reachable via `--transport=http` but no systemd unit ships
# for it — it's a hand-rolled deployment.
if [[ -z "$TRANSPORT" ]]; then
    if [[ "${XDG_CURRENT_DESKTOP:-}" =~ KDE ]] && [[ "${XDG_SESSION_TYPE:-}" == "wayland" ]]; then
        TRANSPORT="unix"
    else
        TRANSPORT="stdio"
    fi
    echo "==> auto-detected transport: $TRANSPORT"
fi
case "$TRANSPORT" in
    unix|stdio) ;;
    http)
        echo "warning: --transport=http is deprecated — Claude Code's HTTP MCP" >&2
        echo "  client routes loopback through HTTPS_PROXY when set, breaking" >&2
        echo "  the wire. install.sh ships only the Unix-socket systemd unit;" >&2
        echo "  for http you'll need to wire systemd yourself. See ADR-0012." >&2
        ;;
    *) echo "error: --transport must be 'unix', 'http' (deprecated), or 'stdio'; got '$TRANSPORT'" >&2; exit 2 ;;
esac

# Run from repo root regardless of caller's cwd.
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

require() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: required command not found: $1" >&2
        exit 1
    }
}
require cargo
require python3
[[ $SKIP_MCP -eq 1 ]] || require claude

BIN="$PREFIX/bin/kde-mcp"
DESKTOP="$HOME/.local/share/applications/kde-mcp.desktop"

echo "==> building and installing $BIN"
cargo install --path . --root "$PREFIX" --force

echo "==> writing $DESKTOP"
mkdir -p "$(dirname "$DESKTOP")"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=KDE Computer-Use MCP Server
GenericName=Desktop Automation Server
Comment=MCP server for KDE Plasma 6 desktop control
Icon=preferences-desktop
TryExec=$BIN
Exec=$BIN
NoDisplay=true
Categories=Utility;Accessibility;

# Authorize this binary to call the org.kde.KWin.ScreenShot2 D-Bus interface.
# KWin's KApplicationTrader check looks up the binary's .desktop file and
# refuses screenshot calls if this list is missing.
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
EOF

echo "==> refreshing desktop / KDE service caches"
command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database "$HOME/.local/share/applications/" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null 2>&1 \
    && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

if [[ "$TRANSPORT" == "unix" ]]; then
    echo "==> installing systemd-user units (ADR-0012)"
    SYSTEMD_DIR="$HOME/.config/systemd/user"
    install -D -m 0644 packaging/kde-mcp.socket  "$SYSTEMD_DIR/kde-mcp.socket"
    install -D -m 0644 packaging/kde-mcp.service "$SYSTEMD_DIR/kde-mcp.service"
    systemctl --user daemon-reload
    # Stop any prior running unit so the new binary is picked up cleanly.
    systemctl --user stop kde-mcp.service 2>/dev/null || true
    systemctl --user enable --now kde-mcp.socket
    SOCKET_PATH="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/kde-mcp/mcp.sock"
    # Force-spawn the service so the socket file is bound and visible.
    for _ in 1 2 3 4 5; do
        if [[ -S "$SOCKET_PATH" ]]; then
            break
        fi
        # Trigger socket activation with a tiny dummy connect attempt.
        timeout 1 bash -c "exec 3<>/dev/tcp/127.0.0.1/0 2>/dev/null" 2>/dev/null
        sleep 0.5
    done
fi

if [[ $SKIP_MCP -eq 0 ]]; then
    echo "==> registering kde-mcp at user scope (transport=$TRANSPORT)"
    claude mcp remove --scope user kde-mcp >/dev/null 2>&1 || true
    case "$TRANSPORT" in
      unix)
        SOCKET_PATH="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/kde-mcp/mcp.sock"
        claude mcp add --transport http --scope user kde-mcp "unix://${SOCKET_PATH}"
        ;;
      http)
        TOKEN_FILE="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/kde-mcp/token"
        if [[ ! -f "$TOKEN_FILE" ]]; then
            echo "warning: HTTP token file missing at $TOKEN_FILE — start the daemon" >&2
            echo "  manually with --transport=http to mint it, then re-run install." >&2
            exit 1
        fi
        TOKEN="$(cat "$TOKEN_FILE")"
        claude mcp add --transport http --scope user kde-mcp http://127.0.0.1:7575/mcp \
            --header "Authorization: Bearer $TOKEN"
        ;;
      stdio)
        register_stdio() {
            local name="$1"; shift
            claude mcp remove --scope user "$name" >/dev/null 2>&1 || true
            claude mcp add --transport stdio --scope user "$name" -- "$BIN" "$@" --transport=stdio
        }
        register_stdio "kde-mcp" --mode=live
        if [[ $WITH_VIRTUAL -eq 1 ]]; then
            echo "==> registering kde-mcp-virtual at user scope"
            register_stdio "kde-mcp-virtual" --mode=virtual --vnc=5910
        fi
        ;;
    esac
fi

if [[ $SKIP_ALLOWLIST -eq 0 ]]; then
    echo "==> merging read-only tool allowlist into ~/.claude/settings.json"
    SERVER_NAMES="kde-mcp"
    [[ $WITH_VIRTUAL -eq 1 ]] && SERVER_NAMES="kde-mcp kde-mcp-virtual"
    SERVER_NAMES="$SERVER_NAMES" python3 - <<'PYEOF'
import json, os, pathlib, sys

p = pathlib.Path.home() / ".claude" / "settings.json"
p.parent.mkdir(parents=True, exist_ok=True)

cfg = {}
if p.exists():
    try:
        cfg = json.loads(p.read_text())
    except json.JSONDecodeError as e:
        print(f"    error: {p} is not valid JSON: {e}", file=sys.stderr)
        sys.exit(1)

READ_ONLY = [
    "observe", "inspect", "screenshot", "read_text", "wait",
    "window_query", "clipboard_read", "text_editor.view",
]
wanted = []
for server in os.environ["SERVER_NAMES"].split():
    wanted += [f"mcp__{server}__{t}" for t in READ_ONLY]

perms = cfg.setdefault("permissions", {})
allow = perms.setdefault("allow", [])
added = [t for t in wanted if t not in allow]
allow.extend(added)

tmp = p.with_suffix(p.suffix + ".tmp")
tmp.write_text(json.dumps(cfg, indent=2) + "\n")
os.replace(tmp, p)

print(f"    {len(added)} new entries, {len(wanted) - len(added)} already present")
PYEOF
fi

echo
echo "==> verifying"
[[ -x "$BIN" ]] && echo "    binary: $BIN ($(stat -c%s "$BIN") bytes)"
[[ -f "$DESKTOP" ]] && echo "    .desktop: $DESKTOP"
if [[ $SKIP_MCP -eq 0 ]]; then
    echo "    MCP entries:"
    claude mcp list 2>&1 | grep -E '^kde-mcp' | sed 's/^/      /'
fi

echo
case "$TRANSPORT" in
    unix)
        SOCKET_PATH="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/kde-mcp/mcp.sock"
        cat <<NOTE
Done — daemon mode, Unix socket (ADR-0012).
  Socket:  $SOCKET_PATH
  Verify:  curl -fsS --unix-socket "$SOCKET_PATH" http://localhost/healthz
  Logs:    journalctl --user -u kde-mcp.service -f
  Status:  systemctl --user status kde-mcp.service kde-mcp.socket
  Stop:    systemctl --user stop kde-mcp.socket
Open a fresh Claude Code session to pick up the new MCP entry.
NOTE
        ;;
    http)
        cat <<'NOTE'
Done — daemon mode, TCP (deprecated).
  Verify:  curl -fsS http://127.0.0.1:7575/healthz
  No systemd unit ships for http — run the daemon manually:
    kde-mcp --mode=live --transport=http
Open a fresh Claude Code session to pick up the new MCP entry.
NOTE
        ;;
    stdio)
        cat <<'NOTE'
Done — stdio mode.
Open a fresh Claude Code session to pick up the new server.
First call will fire a KDE notification ("Claude acquired your desktop"); that's by design.
See docs/MCP-SETUP.md for operational gotchas.
NOTE
        ;;
esac
