# kde-mcp

> An MCP server that lets an AI agent observe and control a KDE Plasma 6 / Wayland desktop through programmatic primitives — the accessibility tree (AT-SPI), KWin Scripting, and libei via KWin's private EIS interface — with screenshots only as a fallback.

**Status:** working MVP. All planned tools are implemented; the server runs as a systemd-user daemon and is driven daily from Claude Code.
**Platform:** KDE Plasma 6 on Wayland (KWin). **Language:** Rust (edition 2024). **License:** MIT.

It is the Linux/KDE counterpart to the `computer-use` capability Anthropic ships for macOS. Anthropic's version is macOS-only; this project targets KDE Plasma 6 on Wayland.

## What makes it different

- **Accessibility-tree first; pixels as fallback.** The primary action is `act_on_element`, which invokes an AT-SPI action on a referenced element — no coordinate hallucination, no input-emulation race, and roughly 5–30× cheaper than screenshot round-trips. Screenshots are an explicit escape hatch, not the default loop. (ADR-0005)
- **Response-as-instruction.** Decision-relevant information is carried in structured response fields, not in tool-description prose — the agent reacts to a `diff` in the moment instead of a paragraph far back in context. (ADR-0004)
- **Native to KDE/Wayland, no portal dialogs.** Input goes through `org.kde.KWin.EIS.RemoteDesktop` (libei), capture through `org.kde.KWin.ScreenShot2`, window control through `org.kde.KWin.Scripting`. No Xvfb, no Docker, no xdotool, no interactive portal prompts.
- **One mode per process.** `--mode=live` drives the real desktop; `--mode=virtual` spawns a headless `kwin_wayland --virtual` session (optionally watchable over VNC) for isolated automation. No runtime mode-swapping, so there is no dual-context misrouting to reason about. (ADR-0003)
- **A deliberately small, safety-shaped tool surface — 12 tools.** Tools consolidate along intent and split along safety, so the host's `readOnlyHint`/`destructiveHint` annotations do the right thing. Four tools that merely duplicated host-native capabilities (`bash`, `text_editor.*`, `wait`) were removed once they failed that test. (ADR-0006, ADR-0013)
- **A safety architecture adapted from Anthropic's design.** Single-session mutation lease, per-app approval via KDE notifications, app-category tiering (view-only / click-only / full), terminal-excluded screenshots, and a `Meta+Esc` global cancel grabbed at the compositor via KGlobalAccel. Every destructive tool routes through a central policy gate. (ADR-0007)

For the full side-by-side with Anthropic's macOS `computer-use`, see [`docs/comparison-with-anthropic-computer-use.md`](docs/comparison-with-anthropic-computer-use.md). A broader, honest comparison against other Linux desktop-control MCP projects is planned (see [`docs/research/prior-art.md`](docs/research/prior-art.md) for the current survey).

## Architecture

- **Three transports** in one binary: stdio, Streamable-HTTP, and a Unix domain socket.
- **Singleton daemon.** The HTTP transport runs as a systemd-user service on `127.0.0.1:7575` with socket activation and idle-stop, so multiple agent sessions share one process (one tray icon, one AT-SPI subscription) instead of contending. The loopback bind is the trust boundary. (ADR-0011, ADR-0012, ADR-0015)
- **Audited dispatch.** Every tool call writes one structured audit record; untrusted external strings are carried with an explicit `trust` tag so the agent can tell apart what it authored from what the screen returned. (ADR-0008)

## Install

See [`packaging/README-install.md`](packaging/README-install.md) for dependencies (Arch / Fedora / Debian), the build, the systemd-user units, and the Claude Code / Claude Desktop MCP configuration. In short:

```sh
cargo install --path .
./packaging/install.sh        # installs the .desktop authorization + systemd-user units
```

Then point your MCP client at `http://127.0.0.1:7575/mcp`.

## Repository layout

```
src/                  Rust implementation (tools, transports, policy gate, AT-SPI / KWin / EIS plumbing)
examples/             Standalone EIS probers — minimal POCs for the libei input path
docs/specs/           The design specification (the load-bearing document)
docs/adr/             Architecture Decision Records (0001–0015) — one decision per file
docs/research/        Snapshot research that informed the design (Wayland/KDE primitives, prior art, LLM tool-design)
docs/plans/           Historical implementation plans (planning artifacts; the code is the source of truth)
docs/runbook/         A machine-executable verification protocol and product scenarios
tools/                Latency analyzer for the per-phase tool-call profiling
packaging/            .desktop authorization, systemd-user units, policy.toml example
```

## Development

Contributors and AI coding agents: read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) before opening a PR. The short version: Conventional Commits, `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` must be clean before every commit, no `unwrap()`/`expect()` on user-reachable paths, and one ADR per non-obvious decision.

## License

MIT — see [`LICENSE`](LICENSE).
