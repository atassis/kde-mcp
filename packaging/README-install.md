# Installation guide

The fastest path is the scripted install. The manual steps below are kept as a reference for people who want to know what's happening under the hood, or who have a non-standard prefix.

## Scripted install (recommended)

After installing system deps (next section), from the repo root:

```sh
./packaging/install.sh                 # build + install + register live-mode + allowlist 8 read-only tools
./packaging/install.sh --with-virtual  # also register the virtual-mode entry (Phase 5 unverified — opt-in)
./packaging/install.sh --skip-mcp      # build only; skip Claude Code config
./packaging/install.sh --help          # all flags
```

To reverse: `./packaging/uninstall.sh` (or `--purge` to also clear `~/.local/state/kde-mcp/`).

See `docs/MCP-SETUP.md` for the full layout, design choices, and operational gotchas.

## Prerequisites

KDE Plasma 6 on Wayland. The binary refuses to run outside this. Check:

```sh
echo $XDG_CURRENT_DESKTOP   # should contain KDE
echo $XDG_SESSION_TYPE      # should be wayland
```

## System dependencies

### Arch (and derivatives)

```sh
sudo pacman -S libei at-spi2-core wl-clipboard wayvnc \
               tesseract tesseract-data-eng \
               spectacle xdg-desktop-portal-kde
```

### Fedora

```sh
sudo dnf install libei at-spi2-core wl-clipboard wayvnc \
                 tesseract tesseract-langpack-eng \
                 spectacle xdg-desktop-portal-kde
```

### Debian / Ubuntu

```sh
sudo apt install libei-bin at-spi2-core wl-clipboard wayvnc \
                 tesseract-ocr tesseract-ocr-eng \
                 kde-spectacle xdg-desktop-portal-kde
```

### Rust toolchain

Install via [rustup](https://rustup.rs) if you don't have it:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Minimum Rust version: 1.78. Newer is fine.

## Build and install the binary

From the repo root:

```sh
cargo install --path . --root ~/.local
```

This puts `kde-mcp` at `~/.local/bin/kde-mcp`. Make sure `~/.local/bin` is on your `PATH`.

## Authorize the ScreenShot2 D-Bus interface

KWin requires the binary to have an installed `.desktop` file declaring its right to call the screenshot interface. From the repo root:

```sh
install -m 644 packaging/kde-mcp.desktop ~/.local/share/applications/
```

This is a one-time setup. Until this is done, `screenshot` tool calls will fail with "process is not authorized to take a screenshot."

## Register with Claude Code

Match the server names the script uses so `~/.claude/settings.json` allowlist entries (`mcp__kde-mcp__*`) line up:

```sh
claude mcp add --transport stdio --scope user kde-mcp \
    -- ~/.local/bin/kde-mcp --mode=live
claude mcp add --transport stdio --scope user kde-mcp-virtual \
    -- ~/.local/bin/kde-mcp --mode=virtual --vnc=5910
```

For Claude Desktop or other MCP hosts, add to your config (e.g. `~/.config/Claude/claude_desktop_config.json`):

```jsonc
{
  "mcpServers": {
    "kde-mcp": {
      "command": "/home/USER/.local/bin/kde-mcp",
      "args": ["--mode=live"]
    },
    "kde-mcp-virtual": {
      "command": "/home/USER/.local/bin/kde-mcp",
      "args": ["--mode=virtual", "--vnc=5910"]
    }
  }
}
```

## Smoke test

Open Claude Code in any directory and ask:

> "Use the kde-mcp MCP server to call `observe` and tell me what window is focused."

If the server is correctly installed and authorized, you'll get a structured snapshot of your desktop.

## Watching the virtual session

The `kde-virtual` server with `--vnc=5910` exposes the headless KWin session over VNC on `localhost:5910`. Open it with KRDC:

```sh
krdc vnc://localhost:5910
```

Or any other VNC client. The connection is bound to localhost only — not exposed to your network.

## Troubleshooting

- **"process is not authorized to take a screenshot"** — the `.desktop` file isn't installed or KWin hasn't refreshed its application cache. Run `kbuildsycoca6` after installing the desktop file.
- **`libei` error on startup** — your distro's `libei` may be older than 1.3. Check `pkg-config --modversion libei` or `ldconfig -p | grep libei`.
- **AT-SPI returns empty trees** — confirm `at-spi2-core` is installed and that the bus is running: `pgrep at-spi-bus-launcher`. Some apps (notably Chromium/Electron) need explicit flags.
- **Virtual mode bootstrap times out** — likely `kwin_wayland --virtual` is missing or the user's `dbus-run-session` isn't available. Both ship with KDE/dbus by default.

## Uninstall

```sh
./packaging/uninstall.sh           # removes binary, .desktop, MCP entries, allowlist
./packaging/uninstall.sh --purge   # also drops ~/.local/state/kde-mcp/
```

Manual:

```sh
claude mcp remove --scope user kde-mcp
claude mcp remove --scope user kde-mcp-virtual  # if registered
rm ~/.local/bin/kde-mcp ~/.local/share/applications/kde-mcp.desktop
kbuildsycoca6 --noincremental
# strip mcp__kde-mcp__* entries from ~/.claude/settings.json by hand
```
