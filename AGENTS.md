# Contributor & agent guide

Guidance for anyone — human or AI coding agent — making changes to kde-mcp. Keep this file thin; detail lives in `docs/`. The code is the source of truth; if a doc contradicts the code, the code wins (open a PR to fix the doc).

## Setup & commands

```sh
cargo build                                              # build
cargo test --workspace                                   # run the test suite
cargo run -- --mode=virtual                              # drive a headless KWin session (safe for dev)
./packaging/install.sh                                   # install systemd-user units + .desktop authorization
```

- Platform: KDE Plasma 6 on Wayland (KWin). The tools talk to live KDE D-Bus interfaces, so most integration behaviour only reproduces on that stack.
- For anything that drives input or windows, develop against `--mode=virtual`, not your real session.
- System dependencies (libei, at-spi2-core, wl-clipboard, tesseract, …) are listed in `packaging/README-install.md`.

## Before every commit (required, non-negotiable)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must be clean. CI will reject anything that isn't.

## Code style

- Edition 2024, stable toolchain.
- Errors: `thiserror` for library types, `anyhow` with `.context(...)` at the binary boundary. Match the taxonomy in the spec (§8).
- **No `unwrap()` or `expect()` on user-reachable paths.** Use `?`. Tool handlers return `Result<_, ToolError>`.
- `tracing`, not `println!`, for diagnostics in library code.
- Unit tests in-file under `#[cfg(test)] mod tests`; integration tests under `tests/`.
- Module layout follows the spec (§13) — don't reorganise on a whim.

## Design principles (non-negotiable — see spec §4 and the ADRs)

1. **Accessibility-tree first; pixels are the fallback.** `act_on_element` is the primary action; `mouse`/`screenshot` are escape hatches. (ADR-0005)
2. **Response-as-instruction.** Put decision-relevant data in structured response fields, not in tool-description prose. Design a tool's response shape before its parameters. (ADR-0004)
3. **Poka-yoke.** Make wrong actions impossible by schema — references, not coordinates, where a reference is required. (ADR-0006)
4. **Every destructive tool routes through the policy gate** (`src/policy.rs`). Never bypass it. (ADR-0007)
5. **A tool earns its place** only if it exposes a primitive the host can't reach natively, or collapses a host-reachable sequence whose response shape carries information the host couldn't otherwise compose. Don't reintroduce `bash`/`text_editor`/`wait` — they were removed for failing this test. (ADR-0013)
6. **KDE Plasma 6 / Wayland only.** No other compositors (ADR-0002). Never shell out to xdotool/scrot/Xvfb — they're X11. Use libei via the KWin EIS fd and `org.kde.KWin.ScreenShot2`.

## Repo etiquette

- **Commits:** Conventional Commits — `type(scope): subject`, subject ≤ 72 chars, types `feat`/`fix`/`chore`/`docs`/`refactor`/`test`. The body explains *why*, not *what* — the diff shows what. No tool/assistant attribution lines in commit messages.
- **Branches & PRs:** work on a feature branch; open a pull request against `main`. Don't force-push shared branches. Don't `git push --force` or `git reset --hard` on anything you didn't author locally.
- **One ADR per non-obvious decision** (`docs/adr/`): status / context / decision / rationale / consequences / alternatives. Never rewrite an accepted ADR — supersede it with a new one and set the old `Status:` to `Superseded by ADR-NNNN`. Keep `docs/adr/README.md` in sync.

## Architecture orientation

- `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` — the design specification (start here).
- `docs/adr/` — the load-bearing decisions, one per file.
- `docs/research/` — the Wayland/KDE primitive landscape, prior art, and LLM tool-design notes that informed the design.

## Gotchas

- Running `--mode=live` fires a KDE notification on startup/shutdown and can emit real input — batch live testing deliberately.
- KWin D-Bus is case-sensitive and version-specific (`org.kde.KWin` vs `org.kde.kwin`; Plasma 6 renamed several scripting globals). See `docs/research/wayland-kde-primitives.md`.
