# Instructions for AI assistants working in this repo

This repository is a **design-phase project** for `kde-mcp`, an MCP server that controls KDE Plasma 6 / Wayland desktops. There is no Rust source code yet. Everything load-bearing is in `docs/`.

If you are an AI assistant (Claude Code, Cursor, Cline, Zed-with-AI, etc.) opening this repo: **read `docs/HANDOFF.md` first**, then this file, then the relevant sections of `docs/specs/2026-05-13-kde-computer-use-mcp-design.md`.

---

## How to operate

### Project state and the next concrete step

- Two commits exist: initial design (`3fdbccf`) and the safety-architecture upgrade (`5d2a943`).
- **The next step in the workflow** is invoking the `superpowers:writing-plans` skill to turn the spec into a sequenced implementation plan, then `superpowers:executing-plans` or `superpowers:subagent-driven-development` to actually write code.
- Do **not** start writing Rust code without first producing the plan via `writing-plans`. The brainstorming skill that produced this design explicitly mandates the plan step.
- Before invoking `writing-plans`, the user (atassis) should explicitly approve the spec. If they haven't, ask first.

### Doc layout (you should know without re-reading)

```
docs/
├── specs/2026-05-13-kde-computer-use-mcp-design.md  # the source of truth
├── research/                                        # snapshot research that informed the design
│   ├── anthropic-computer-use-reference.md
│   ├── wayland-kde-primitives.md
│   ├── prior-art.md
│   └── llm-tool-design-principles.md
├── adr/                                             # ADRs 0001-0007; never delete or rewrite history-style
└── comparison-with-anthropic-computer-use.md        # honest side-by-side
```

### Design principles — non-negotiable

From `docs/specs/...md` §4 (and corresponding ADRs):

1. **Accessibility-tree first; pixels as fallback** (ADR-0005). `act_on_element` is the primary action. `mouse`/`screenshot` are explicit escape hatches.
2. **Response-as-instruction** (ADR-0004). Decision-relevant info goes in structured response fields, not in tool description prose. If you add a tool, design its response shape before its parameters.
3. **High signal-to-noise per token.** Strip ceremonial fields. Omit empty arrays. If a field is rarely the answer, leave it out by default.
4. **Progressive disclosure.** Default cheap; let detail be opt-in (`detail: "summary"|"interactive"|"full"`).
5. **Poka-yoke — wrong actions impossible by schema.** Refs only; no coordinates where a ref is required. Splits along safety (`type` vs `key`); consolidation along intent (`act_on_element`).
6. **Just-in-time references.** Big payloads return as `resource_link` to a file; small payloads inline.
7. **Quarantine untrusted content.** Every external string carries `trust: "trusted" | "agent_authored" | "external"`.

The tensions between these are documented in §4.8 of the spec. Don't rederive — read.

### ADR conventions

- One ADR per non-obvious decision. Format in the existing ones (status, context, decision, rationale, consequences, alternatives).
- Never rewrite an accepted ADR. If you change a decision, write a new ADR that supersedes it and update the `Status:` field of the old one to `Superseded by ADR-NNNN`.
- ADR numbers are sequential. The index at `docs/adr/README.md` must stay in sync.

### Commit conventions

- Subject line ≤ 72 chars, format: `type(scope): subject` (`feat`, `fix`, `chore`, `docs`, `refactor`).
- Body explains *why*, not *what* — the diff shows what.
- Don't amend an accepted commit unless rebasing local work that isn't pushed.
- The git author should be `atassis <atassikay38@gmail.com>` (the project owner). Do not push to a remote without explicit ask.

### Rust style (when implementation begins)

- Edition 2021, Rust 1.78+.
- One async runtime (`tokio::main`). All I/O async.
- Errors via `thiserror` for libraries, `anyhow` at the binary boundary. Match the taxonomy in §8 of the spec.
- No `unwrap()` on user-reachable paths. `?` everywhere; tool handlers return `Result<ToolResult, ToolError>`.
- Module layout matches §13 of the spec exactly. Don't reorganise on a whim.
- Tests under `tests/` integration; unit tests in-file via `#[cfg(test)] mod tests`.
- `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` clean before any commit.

### What to never do

- Don't write `unwrap()` or `expect()` in tool handlers.
- Don't add a screenshot-after-every-action pattern. That's Anthropic's design; we explicitly rejected it (ADR-0004).
- Don't add prose to tool descriptions where a response field would do (ADR-0004).
- Don't expand scope to other compositors (ADR-0002). KDE Plasma 6 Wayland only.
- Don't bypass the policy gate (`src/policy.rs` once it exists) for destructive tools. Every destructive tool routes through the gate.
- Don't shell out to xdotool/scrot/Xvfb. They're X11; we're Wayland-native. Use libei via the KWin EIS fd and `org.kde.KWin.ScreenShot2`.
- Don't run `git push --force`, `git reset --hard`, or any destructive git op without explicit user confirmation (per user's global rules at `~/.claude/CLAUDE.md`).
- Don't use `sudo` directly in commands; provide the command for the user to run manually if root is needed (per user's global rules).
- Don't push to GitHub or any remote unless the user explicitly asks. This is a personal-use project.

### Skills to invoke (Claude Code specific)

- `superpowers:writing-plans` — turn this spec into an implementation plan. **This is the next step.**
- `superpowers:executing-plans` — execute the plan with review checkpoints.
- `superpowers:test-driven-development` — for each tool's implementation.
- `superpowers:systematic-debugging` — when something doesn't work.
- `superpowers:verification-before-completion` — before claiming any phase is done.
- `superpowers:code-reviewer` — after each major phase.
- `superpowers:writing-skills` — only if you find yourself writing a generic pattern more than twice.

Do not invoke `frontend-design`, `mcp-builder`, or other implementation skills before `writing-plans` produces the plan. The brainstorming flow terminates at writing-plans, not at any specific implementation skill.

### Environment expectations

The development machine is the user's daily-driver KDE Plasma 6 Wayland workstation. Assume:
- `cargo` installed, Rust 1.78+
- `libei`, `at-spi2-core`, `wl-clipboard`, `wayvnc`, `tesseract`, `spectacle`, `xdg-desktop-portal-kde` installable via the system package manager
- The user runs against their real session; be careful with destructive actions during testing (use `--mode=virtual` for integration tests)
- `~/.config/kde-mcp/policy.toml` does not exist yet; will be created in implementation Phase 3

### When in doubt

- Read the relevant ADR. If no ADR covers it, ask the user before deciding.
- Pattern after kwin-mcp's implementation where the design overlaps (it's Python, but the D-Bus dance is the same).
- The user pushes back. Encourage it. Disagreement is fine; sycophancy is worse.
- Cite the spec section in your reasoning when you make a non-trivial design call ("per §4.5 poka-yoke, this should…").

### When the user says they don't remember context

Read `docs/HANDOFF.md`, then summarise the current state in 5 sentences before doing anything else.
