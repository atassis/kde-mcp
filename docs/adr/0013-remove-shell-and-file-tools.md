# ADR-0013: Remove `bash`, `text_editor_*`, and sleep-only `wait` from the tool catalogue

**Status:** Accepted (2026-05-16)

Supersedes the `bash`, `text_editor_view`, `text_editor_edit`, and `wait` entries in [[0006-tool-surface-shape]] and in `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` §6. Reduces the wire-name surface from 16 to 12 (15 → 11 logical tools).

## Context

Reviewing the catalogue against a sharper framing of what this MCP is for: **an MCP server exists to give the host capabilities the host does not already have, or to collapse a difficult-but-repeatable sequence into a single call. It does not exist to wrap commands the host can already run.** The current `bash` tool prompted the audit — but applying the rule honestly catches three more tools that fail the same test.

Concrete checks against each candidate:

- **`bash`.** Claude Code (the host) ships its own bash. `mcp__kde-mcp__bash` is a parallel persistent PTY with a different cwd/env, so the agent has two shells with divergent state — strictly worse than one. The tool's `openWorldHint: true` annotation in spec §11 already marks it as the highest-risk surface. The 2026-05-15 L2.15 runbook failure (PTY-sentinel parser leaking prompt + sentinel into stdout, `exit_code: null`) is itself evidence: a generic shell escape hatch is the riskiest, most test-shaped surface in the binary, and we are paying maintenance cost on it for no capability the host doesn't already have.

- **`text_editor_view` and `text_editor_edit`.** Both take `path: String` (absolute, enforced at the schema layer per spec §89) and operate on the filesystem. Implementations at `src/tools/text_editor.rs:58` and `:193` confirm: they're file I/O, not manipulation of text fields in running apps. Claude Code's native `Read` / `Edit` / `Write` cover the same ground. The kde-mcp value-add would be trust-tagging file contents as `external`, but that's a host-side audit concern handled by the host's tool annotations, not a reason for a parallel implementation. Typing into text fields **inside running apps** is covered by `keyboard.type` (libei keystroke injection) and `act_on_element` (AT-SPI `EditableText.InsertText`) — both of which *are* primitives the host can't reach.

- **`wait` in its current sleep-only form.** `src/tools/wait.rs:56` shows the implementation: a sleep loop on a `condition` argument whose first branch is `"seconds"`. It is dispatched as an observation tool (`src/lease.rs:309` classifies the read-only set; `wait` goes through `dispatch_audited` and refreshes the ADR-0011 lease the same way any other read-only call does). There is no dedicated "extend the session lock" tool because every read-only call already extends it. So `wait { seconds: N }` adds nothing over `observe { detail: "summary" }`, except it returns less useful information. A `wait` that *would* earn its keep is one that subscribes to a KWin signal (`windowAdded` / `windowActivated` / AT-SPI focus event) and returns when a named condition holds — that is a real primitive the host can't reach via shell. Not implemented today.

## Decision

**Cut four tools from the catalogue:**

| Wire name | Status | Rationale (one line) |
|---|---|---|
| `bash` | Removed | Duplicates host's bash. |
| `text_editor_view` | Removed | Duplicates host's `Read`. |
| `text_editor_edit` | Removed | Duplicates host's `Edit` / `Write`. |
| `wait` | Removed (deferred re-add) | Sleep-only form is equivalent to `observe`; signal-driven condition wait may return later. |

**Catalogue after this ADR:**

| # | Wire name | Kind | Primitive |
|---|---|---|---|
| 1 | `observe` | observe | KWin + AT-SPI composed snapshot |
| 2 | `inspect` | observe | AT-SPI tree walk |
| 3 | `screenshot` | observe | `org.kde.KWin.ScreenShot2` D-Bus + per-window capture |
| 4 | `read_text` | observe | OCR over in-process screenshot pipeline |
| 5 | `window_query` | observe | KWin scripting via caller-hosted D-Bus sink |
| 6 | `clipboard_read` | observe | `wl-paste` + trust-tagged response |
| 7 | `mouse` | mutate | libei mouse injection through KWin EIS fd |
| 8 | `keyboard.type` | mutate | libei keystroke injection |
| 9 | `keyboard.key` | mutate | libei chord injection |
| 10 | `act_on_element` | mutate | AT-SPI `Action.DoAction` / `EditableText.InsertText` |
| 11 | `window_control` | mutate | KWin scripting (resize, move, close, minimise) |
| 12 | `clipboard_write` | mutate | `wl-copy` + agent-authored trust tag |

12 wire tools; 11 logical (`keyboard` is one logical tool with a `{type, key}` split for poka-yoke per ADR-0006).

`clipboard_read` and `clipboard_write` are kept on the borderline-call principle: `wl-paste` / `wl-copy` exist as shell commands, but the kde-mcp value-add is the wire-format `trust: "external" | "agent_authored"` discriminator (spec §4.7) that the host's prompt-injection defence relies on. Tool descriptions should make the trust-tagging contract explicit so the agent picks them over a shell `wl-paste`. `read_text` is kept on the same grounds: `tesseract` is a shell command, but the value-add is feeding directly from the in-process screenshot pipeline and emitting the same trust tag.

`screenshot` is kept despite `spectacle --window` existing because (a) spectacle can't target an opaque kde-mcp window ref, and (b) the terminal-mask defence in spec §11 lives in our code path.

## Rationale

The MCP-purpose rule we're codifying:

> An MCP tool earns inclusion only if (a) it exposes a primitive the host cannot reach natively, or (b) it collapses a multi-step host-reachable sequence into a single atomic call whose response shape carries decision-relevant information the host couldn't otherwise compose. Pure wrappers around host-reachable commands do not earn inclusion.

Applied retroactively, this kills the four tools above. Applied prospectively, it kills the speculative `app_launch(desktop_id=…)`, `kde_config_get/set`, `process_signal(name=…)` tools that were proposed during the audit conversation — those are pure wrappers around `gtk-launch`, `kreadconfig`, and `kill`, all of which the host's bash already runs.

The rule does **not** kill the borderline-call tools listed above (`clipboard_*`, `read_text`) because their wire-format trust-tagging is a structural defence the host cannot synthesise after the fact: by the time bash returns OCR text or clipboard content as a string, the host has no way to know whether to flag it as `external`.

## Consequences

**Positive:**

- Smaller catalogue: 12 wire tools instead of 16. Each remaining tool corresponds to a primitive the host cannot reach via shell. The "why is this here" question has a one-line answer per tool.
- Cuts the highest-risk surface in the binary. `bash` was the only `openWorldHint: true` tool; after this ADR, no tool carries that annotation. Removes the L2.15 PTY-sentinel parser, the blocklist scaffolding (`check_blocklist`), the sensitive-path warning machinery for `bash` paths, the `bash` audit-log entries with verbatim command strings, and the `BashSession` PTY lifetime tied to the daemon.
- Cuts the `text_editor_*` machinery (read-line-range, str_replace with old/new-hash audit redaction, sensitive-path warnings on file edits). Path-validation logic in `src/tools/text_editor.rs` and its tests retire.
- Tool selection ambiguity goes away: when the agent wants to run a shell command or edit a file, the host's native tool is the only choice. No more "should I use mcp__kde-mcp__bash or my built-in bash?"
- The four tests in `tests/integration/bash_pty.rs` (added 2026-05-16) get deleted along with `src/tools/bash.rs`. The PTY-sentinel fix from earlier today is reverted — fixing a tool we're removing was wasted work, but the diagnosis fed directly into this ADR.

**Negative / what we give up:**

- The spec §4.4 hint *"virtual session has no running apps; use bash to launch one"* loses its referent. The fix is to drop that hint text from the `observe` virtual-mode response; the agent will surface the same problem via "no windows visible" and the host can decide whether to call its own bash to launch something.
- `wait { seconds: N }` for explicit "give the previous action time to settle" is no longer a no-op tool — the agent has to call `observe` instead, which does the same lease-refresh and returns useful state. Net positive on information density, possible minor friction.
- Removing `text_editor_edit` removes our sensitive-path warnings (spec §11.9) for file edits. Those warnings move to the host's native edit tool (out of scope for this ADR) or are simply lost. We accept the loss: the host's own permission model is the load-bearing defence, not a parallel warning string in a competing tool's response.

**Neutral / housekeeping:**

- Spec §6 tool catalogue rewritten to the 12-tool list above. The table at spec §13.x (tool index) drops four rows.
- ADR-0006 status updated to `Superseded in part by ADR-0013` with a note that the surface-shape principles still hold for the remaining 12 tools.
- The runbook (`docs/runbook/runs/2026-05-15-1909/L2.15-bash.json`) becomes a historical artefact — the failure it documents is moot because the tool is gone.
- `docs/comparison-with-anthropic-computer-use.md` updates the side-by-side: Anthropic's `computer-use` includes `bash` and `text_editor` (Claude Computer Use API parity); we explicitly diverge because the host already provides them. That divergence is worth calling out as a design difference, not as a parity gap.

## Alternatives considered

1. **Keep `bash` but tighten the blocklist.** Rejected: doesn't address the duplication. The blocklist is mitigation for a tool that shouldn't exist in this MCP.
2. **Keep `text_editor_*` for the audit-log-with-trust-tagging value.** Rejected: trust-tagging is a wire-format concern, and the host's native tools already log to the host's audit pipeline. Maintaining two parallel file-edit code paths to get a tag that nobody downstream reads is not worth the surface.
3. **Redesign `wait` as a KWin-signal condition wait in this ADR.** Rejected as scope creep: the *removal* is the load-bearing decision here. A new condition-wait tool, if we want one, deserves its own ADR with the signal-set, the timeout/closest-match response shape, and the lease-refresh cadence worked out separately. Punted to a future ADR if and when a concrete need appears.
4. **Cut `clipboard_*` and `read_text` too, for symmetry.** Rejected: the trust-tagging wire format is a structural defence the host can't synthesise after the fact (rationale section above). They earn their keep on different grounds than `bash` did.

## Implementation notes (for the follow-up commit, not this ADR)

- Delete `src/tools/bash.rs`, `src/tools/text_editor.rs`, `src/tools/wait.rs`.
- Remove the corresponding `pub mod` lines in `src/tools/mod.rs`.
- Remove the rmcp `#[tool]` shims in `src/server.rs` and their `tool_router!` registrations.
- Remove `BashSession`-related fields from `KdeMcp` (`bash_session: Arc<Mutex<Option<BashSession>>>`).
- Remove `tests/integration/bash_pty.rs`, `tests/integration/text_editor.rs`, and any `wait` cases in `tests/integration/tools_list.rs`.
- Update `tests/integration/tools_list.rs::server_advertises_sixteen_tools_with_expected_names` to advertise twelve.
- Remove `BashArgs`/`BashResponse`/`TextEditor*Args`/`Wait*Args` JSON-schema surface; check no resource_link generators referenced these types.
- Strip §6 entries from the spec; update §4.4 to drop the "use bash to launch one" hint; update §11.8 (blocklist) to note it's retired with bash.
- Mark the L2.15 bug (bash PTY parser) moot — the tool it affected no longer exists.

A clean implementation should be < 500 lines net deletion across `src/` and `tests/`, plus the spec/docs updates.
