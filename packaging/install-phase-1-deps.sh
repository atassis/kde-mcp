#!/usr/bin/env bash
# Install system packages needed for kde-mcp Phase 1 live verification.
#
# Run as your normal user; the script invokes `sudo pacman` once per group
# and prompts for your password through sudo's own TTY interaction. Re-runnable
# (uses --needed) — no-op if everything is already installed.
#
# Generated 2026-05-14 after the autonomous Phase 1 run.
set -euo pipefail

if ! command -v pacman >/dev/null 2>&1; then
    echo "error: pacman not found — this script is for Arch / CachyOS." >&2
    exit 1
fi

echo "==> Phase 1 (required) — wl-clipboard"
echo "    Needed for: clipboard_read tool live smoke verification."
sudo pacman -S --needed --noconfirm wl-clipboard

echo
echo "==> Phase 4 (optional, not blocking) — tesseract + English/Russian data"
echo "    Needed for: read_text tool (OCR) when Phase 4 lands."
echo "    Skip with Ctrl-C if you'd rather install when Phase 4 starts."
read -rp "    Install now? [y/N] " ans
case "${ans:-n}" in
    [yY]*)
        sudo pacman -S --needed --noconfirm tesseract tesseract-data-eng tesseract-data-rus
        ;;
    *)
        echo "    Skipped tesseract. Re-run this script later or install manually."
        ;;
esac

echo
echo "==> Verification"
for cmd in wl-paste wl-copy; do
    if command -v "$cmd" >/dev/null 2>&1; then
        echo "    $cmd: $(command -v "$cmd")"
    else
        echo "    $cmd: MISSING (something went wrong)" >&2
    fi
done

echo
echo "Done. clipboard_read live smoke should now work."
