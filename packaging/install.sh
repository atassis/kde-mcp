#!/usr/bin/env bash
# Install kde-mcp into ~/.local/ and wire it into Claude Code at user scope.
#
# Idempotent — safe to re-run after rebuilds or config changes.
#
# Usage:
#   ./packaging/install.sh                  # binary + live-mode MCP + allowlist (default)
#   ./packaging/install.sh --with-virtual   # also register virtual-mode entry
#   ./packaging/install.sh --skip-mcp       # build/install binary only; no Claude config
#   ./packaging/install.sh --skip-allowlist # register MCP but don't touch settings.json
#   ./packaging/install.sh --prefix=/usr/local  # custom install prefix (needs writable dest)
#
# See docs/MCP-SETUP.md for what each step does and why.

set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
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
        -h|--help)         usage; exit 0 ;;
        *)                 echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

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

if [[ $SKIP_MCP -eq 0 ]]; then
    register_server() {
        local name="$1"; shift
        # remove-then-add makes this idempotent without parsing existing config
        claude mcp remove --scope user "$name" >/dev/null 2>&1 || true
        claude mcp add --transport stdio --scope user "$name" -- "$BIN" "$@"
    }
    echo "==> registering kde-mcp (live) at user scope"
    register_server "kde-mcp" --mode=live
    if [[ $WITH_VIRTUAL -eq 1 ]]; then
        echo "==> registering kde-mcp-virtual at user scope"
        register_server "kde-mcp-virtual" --mode=virtual --vnc=5910
    fi
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

cat <<'NOTE'

Done. Open a fresh Claude Code session to pick up the new server.
First call will fire a KDE notification ("Claude acquired your desktop"); that's by design.
See docs/MCP-SETUP.md for operational gotchas.
NOTE
