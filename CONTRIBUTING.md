# Contributing to kde-mcp

Thanks for your interest. This is a small project with a strong, opinionated design — contributions are welcome as long as they fit it.

## Before you start

- For anything more than a typo or an obvious bug fix, **open an issue first** to discuss the approach. The design is documented in `docs/specs/` and `docs/adr/`; a change that cuts against an accepted ADR needs a new ADR that supersedes it, not a quiet edit.
- The platform is KDE Plasma 6 on Wayland. Most integration behaviour only reproduces there.

## Development rules

The full development guide — build commands, the pre-commit quality gate, code style, the design principles, and the ADR workflow — lives in [`AGENTS.md`](AGENTS.md). Please read it before opening a PR. The essentials:

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` must all be clean before every commit.
- Conventional Commits (`type(scope): subject`); commit bodies explain *why*.
- No `unwrap()`/`expect()` on user-reachable paths.
- Every destructive tool routes through the policy gate — never bypass it.

## Pull requests

- Branch off `main`, keep the PR focused, and describe what changed and why.
- Add or update tests for behaviour changes; update the relevant doc/ADR if you change a decision.
- By submitting a PR you agree to license your contribution under the project's [MIT License](LICENSE).

## Code of conduct

Be respectful and constructive. Harassment or abuse isn't tolerated.
