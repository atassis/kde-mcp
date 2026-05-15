#!/usr/bin/env bash
# Reverse what packaging/install.sh did.
#
# Usage:
#   ./packaging/uninstall.sh           # remove MCP entries, binary, .desktop, allowlist
#   ./packaging/uninstall.sh --purge   # also remove ~/.local/state/kde-mcp/ (audit log, approvals)
#   ./packaging/uninstall.sh --prefix=/usr/local
#
# Idempotent — safe to re-run.

set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
PURGE=0

usage() { sed -n '2,/^$/p' "$0" | sed 's/^# \?//'; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --purge)       PURGE=1;            shift ;;
        --prefix=*)    PREFIX="${1#*=}";   shift ;;
        -h|--help)     usage; exit 0 ;;
        *)             echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

BIN="$PREFIX/bin/kde-mcp"
DESKTOP="$HOME/.local/share/applications/kde-mcp.desktop"

if command -v claude >/dev/null 2>&1; then
    echo "==> removing MCP entries"
    for name in kde-mcp kde-mcp-virtual; do
        if claude mcp remove --scope user "$name" >/dev/null 2>&1; then
            echo "    removed: $name"
        fi
    done
fi

echo "==> stripping kde-mcp allowlist entries from ~/.claude/settings.json"
python3 - <<'PYEOF'
import json, os, pathlib

p = pathlib.Path.home() / ".claude" / "settings.json"
if not p.exists():
    raise SystemExit(0)
try:
    cfg = json.loads(p.read_text())
except json.JSONDecodeError:
    raise SystemExit(0)

perms = cfg.get("permissions")
if not isinstance(perms, dict):
    raise SystemExit(0)
allow = perms.get("allow")
if not isinstance(allow, list):
    raise SystemExit(0)

before = len(allow)
kept = [e for e in allow if not (isinstance(e, str) and e.startswith("mcp__kde-mcp"))]
if len(kept) == before:
    raise SystemExit(0)

if kept:
    perms["allow"] = kept
else:
    perms.pop("allow", None)
if not perms:
    cfg.pop("permissions", None)

tmp = p.with_suffix(p.suffix + ".tmp")
tmp.write_text(json.dumps(cfg, indent=2) + "\n")
os.replace(tmp, p)
print(f"    removed {before - len(kept)} entries")
PYEOF

echo "==> removing binary and .desktop"
rm -fv "$BIN" "$DESKTOP" 2>/dev/null | sed 's/^/    /' || true

echo "==> refreshing desktop / KDE service caches"
command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database "$HOME/.local/share/applications/" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null 2>&1 \
    && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

if [[ $PURGE -eq 1 ]]; then
    echo "==> --purge: removing ~/.local/state/kde-mcp/"
    rm -rfv "$HOME/.local/state/kde-mcp/" | sed 's/^/    /' || true
fi

echo
echo "Done."
