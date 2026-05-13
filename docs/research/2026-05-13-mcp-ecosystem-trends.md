# MCP Ecosystem & Agent-Workflow Trends, Late-2025 → Mid-2026

Snapshot research for the kde-mcp plan, covering what's new since the spec was written (2026-05-13). Scope: MCP spec, Anthropic agent-tool guidance, computer-use updates, KDE Plasma 6.5/6.6 surface changes, prior-art deltas. Not a re-derivation of `llm-tool-design-principles.md` — only deltas matter here.

---

## TL;DR

1. **MCP spec moved to `2025-11-25`.** New: Tasks primitive (experimental), URL-mode elicitation, sampling-with-tools, OIDC discovery, icons metadata, tool-error self-correction. Our spec still cites `2025-06-18` — update before implementation begins.
2. **Anthropic published two posts that directly challenge a default we share with most MCP servers**: "Code execution with MCP" (Nov 4 2025) argues most servers shouldn't be called tool-by-tool at all — they should be navigated as a filesystem-shaped programmatic API. Token reduction quoted at **98.7%**.
3. **`advanced-tool-use-2025-11-20` beta** ships three concrete features we should mirror in tool design: tool-search-tool (lazy schema loading), programmatic tool calling (server-side script execution), tool-use examples (a new optional field per tool). The accuracy-on-parameter jump from 72 → 90 % is the load-bearing data point.
4. **kwin-mcp shipped v0.7.0 (Mar 29 2026)** with live-session mode (`session_connect`) and dual-mode (`--default-live-session`) — the architecture is now closer to what kde-mcp wants than what it was at spec-time. **Issue #29 (libei 1.5 segfault) is still open**; #8 and #14 are partially addressed but open. This is a real risk for our `eis` module.
5. **VPI-Bench (Cao et al., Jun 2025 / updated 2026) measures CUAs at up to 51 % attack success on visual prompt injection**, and "existing defense methods offer only limited improvements." Our `trust: trusted | agent_authored | external` discriminator is the right shape, but the field has moved toward **layered hardening + retrieval-gated tools** — worth adding a follow-on ADR.

---

## Bucket 1 — MCP spec & ecosystem updates since 2025-06

### Spec revision: `2025-06-18` → `2025-11-25`

Released 2025-11-25 as the one-year-anniversary spec. Sources:
- [MCP Spec 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [Changelog](https://modelcontextprotocol.info/specification/2025-11-25/changelog/)
- [One Year of MCP: November 2025 Spec Release](https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/)

Material changes relative to the spec we cite (`2025-06-18`):

| Change | Bearing on kde-mcp |
|---|---|
| **Tasks primitive (experimental)** — durable request tracking with polling, deferred result retrieval, states `working / input_required / completed / failed / cancelled`. | Directly applicable to `app.launch`, `window.wait_for`, `eis.replay_macro`. Today our spec models these as long-running tool calls returning when done; Tasks would let us return immediately with a token and let the agent poll. **Worth adopting.** |
| **URL-mode elicitation** — server hands the client an OAuth/consent URL; user authenticates out-of-band; credentials stay server-side. | Not relevant for personal-use kde-mcp. Skip. |
| **Sampling with tools** — server-side agentic loops with `tools` and `toolChoice` parameters on `sampling/createMessage`. | We deliberately rejected server-side sampling in `llm-tool-design-principles.md` "Things deliberately NOT adopted." Re-evaluate? Probably no — but worth re-reading if we ever add `task.summarise_recent_screenshots`. |
| **Icons metadata for tools, resources, prompts, resource templates** | Cosmetic. Skip for MVP. |
| **Tool execution errors used for self-correction** — input-validation errors now flow back as Tool Execution Errors rather than RPC errors, so the model can correct itself in the next turn. | Aligns with our §4.2 response-as-instruction principle. The plan should specify that `kde_mcp::error::ToolError` maps to MCP Tool Execution Errors, not transport-layer errors, except for protocol violations. |
| **OIDC discovery, OAuth Client ID Metadata Documents, incremental scope consent** | Not relevant for personal-use MCP-over-stdio. Skip. |
| **Standardized tool naming guidance (SEP-986)** | Sanity-check: our tools (`screen.capture`, `act_on_element`, `window.move`) use `snake.case` dotted style. Confirm against the spec text before locking the catalogue. |
| **JSON Schema 2020-12 as default dialect** | Already implied. Mention in §8 of the spec when porting. |
| **`description` on `Implementation` interface** | Use for the server's banner. Cheap. |

### Major-host capability updates

- **Claude Code sandboxing landed Oct 2025** as the "Beyond permission prompts" sandbox-runtime: Linux bubblewrap + macOS seatbelt, filesystem + network isolation, **applies to MCP servers** spawned from the sandbox. Library released as `anthropic-experimental/sandbox-runtime` (Apache-2). Internal data: **84 % reduction in permission prompts**.
  - Source: [Beyond permission prompts (claude.com)](https://claude.com/blog/beyond-permission-prompts-making-claude-code-more-secure-and-autonomous), [esokia.com Linux deep-dive](https://labs.esokia.com/post/sandboxing-claude-code-cli-linux-bubblewrap/).
  - Implication: kde-mcp should be **explicitly compatible** with running inside the sandbox-runtime, which means: no shelling out to GUIs (already a constraint), no spawning `xdg-open` for confirmation (use elicitation instead), and the policy.toml needs a "running-under-sandbox" mode that suppresses our own audit-log destinations Claude can't write to.

### Notable MCP servers that overlap

- **microsoft/playwright-mcp 0.0.64 → 0.0.75** (Feb 6 → May 7 2026). Key deltas:
  - `browser_drop` tool for drag-and-drop completion (v0.0.71, Apr 27).
  - Renamed `browser_run_code` → `browser_run_code_unsafe` (v0.0.72, Apr 30). **Direct poka-yoke precedent**: when a tool's safety profile changes, rename it loudly so the model and host both notice.
  - `browser_network_requests` redesigned with indexed access + `responseBody`/`responseHeaders` opts.
  - Session lifecycle simplified — no more `stopped` state; sessions are "running or gone." Mirrors what kwin-mcp did in v0.5.0.
  - Source: [microsoft/playwright-mcp releases](https://github.com/microsoft/playwright-mcp/releases).
- **Microsoft also released `@playwright/cli`** as an MCP-alternative: same capabilities exposed as a CLI, "114k tokens via MCP vs 27k via CLI (~4× reduction)." This is the practical embodiment of Anthropic's "code execution with MCP" pattern.
  - Source: [Playwright MCP changes the build vs. buy equation (bug0.com)](https://bug0.com/blog/playwright-mcp-changes-ai-testing-2026).
- **trycua/cua**: no major architectural shift relevant to a host-Linux server; still VM/container-centric.

---

## Bucket 2 — LLM tool-design & agent-workflow patterns

### New Anthropic posts (Sep 2025 → May 2026)

From [anthropic.com/engineering](https://www.anthropic.com/engineering):

| Date | Title | Bearing on kde-mcp |
|---|---|---|
| Sep 11 2025 | Writing effective tools for agents — with agents | The post `llm-tool-design-principles.md` already cites. No new ground. |
| Sep 17 2025 | A postmortem of three recent issues | Safety-incident postmortem; not tool-design-load-bearing. |
| Sep 29 2025 | [Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) | Already in `llm-tool-design-principles.md`. |
| Oct 16 2025 | [Equipping agents for the real world with Agent Skills](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills) | **Skills are complementary to MCP**, not a replacement. A skill packages procedural knowledge; an MCP server exposes raw capability. Relevant: we should not bundle a "how to use kde-mcp" skill into the server itself — leave that for the host. |
| Oct 20 2025 | [Beyond permission prompts: making Claude Code more secure and autonomous](https://claude.com/blog/beyond-permission-prompts-making-claude-code-more-secure-and-autonomous) | See above (Bucket 1). |
| Nov 4 2025 | [Code execution with MCP: Building more efficient agents](https://www.anthropic.com/engineering/code-execution-with-mcp) | **Most load-bearing new post.** See §"Code execution with MCP" below. |
| Nov 24 2025 | [Introducing advanced tool use on the Claude Developer Platform](https://www.anthropic.com/engineering/advanced-tool-use) | Tool Search Tool, Programmatic Tool Calling, Tool Use Examples. See below. |
| Nov 26 2025 | [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) | The "progress file" pattern. Less load-bearing for an MCP server, but the **end-to-end verification at session start** point is repeated in the official computer-use docs as a tip. |
| Jan 9 2026 | Demystifying evals for AI agents | Eval-methodology focus. Implementation-phase reading. |
| Jan 21 2026 | Designing AI-resistant technical evaluations | Not relevant. |
| Mar 24 2026 | Harness design for long-running application development | Same family as Nov 26 — long-running agent harness patterns. |
| Apr 8 2026 | Scaling Managed Agents: Decoupling the brain from the hands | Anthropic-platform-internal; describes brain/hands split. Worth a single read but not load-bearing. |
| Apr 23 2026 | An update on recent Claude Code quality reports | Not relevant. |

### Code execution with MCP — the big one

[Anthropic — Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp), 2025-11-04.

Core argument:
> Traditional MCP loads all tool definitions upfront and round-trips every intermediate result through the model. "From 150,000 tokens to 2,000 tokens — a time and cost saving of 98.7%."

The pattern they advocate: **agents write code that calls the MCP server as a programmatic API**, with tool schemas discovered on demand by exploring a filesystem-shaped layout. The model loads only the schemas it actually needs, and intermediate data can be filtered in-environment before any value reaches the context window.

Our spec doesn't model this — `act_on_element` etc. are individual tools called by the model. **For a 15-tool surface this is probably fine**; the post explicitly frames the savings as "hundreds or thousands of tools." But:

- Two of our tools — `app.list_running` (potentially big) and `screen.capture` with `detail: "full"` — already hit the resource_link pattern, which is the same idea expressed inline.
- Our biggest token risk is multi-step macros (`eis.replay_macro` over a long sequence). The right MVP response is **not** to add programmatic tool calling — it's to make sure macro recording stays server-side and replay returns deltas, not per-step screenshots. Already in the spec.

### Advanced tool use (Nov 24 2025)

Beta `advanced-tool-use-2025-11-20`. Three features:

1. **Tool Search Tool** — model discovers tools by search rather than reading every schema in the system prompt. Mirrors "code execution with MCP" but inside the existing tool-call protocol.
2. **Programmatic Tool Calling** — model emits a Python-shaped script that runs in a sandboxed runtime invoking tools directly. **"Cuts token usage by 37% and reduces latency."**
3. **Tool Use Examples** — optional `examples: [...]` field per tool, demonstrating call shapes. Anthropic measured **72 % → 90 % accuracy on parameter handling**.

Source: [Anthropic — Introducing advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use). Beta header: `advanced-tool-use-2025-11-20`.

Implication for kde-mcp:
- Tool Search and Programmatic Tool Calling are client-side features — irrelevant to us as a server.
- **Tool Use Examples is server-side**, and trivially cheap. Every kde-mcp tool should ship `examples` in its annotations. The 18-point accuracy delta on parameter handling is the single highest-leverage change in this whole document.

### New patterns from other labs / practitioners

- **Microsoft Semantic Kernel RCE postmortem (May 7 2026)**: prompt-injection-to-RCE chain in their Python SDK. Lessons: every tool that runs code (our `bash` doesn't exist, but `dev.exec` if we ever add one) must run inside a process boundary the host trusts.
  - Source: [Microsoft Security Blog — When prompts become shells](https://www.microsoft.com/en-us/security/blog/2026/05/07/prompts-become-shells-rce-vulnerabilities-ai-agent-frameworks/).
- **Microsoft `@playwright/cli`** (mentioned above) is the "skip MCP entirely" answer for token efficiency. Personal note: not something kde-mcp should ape, but worth being aware of for users who care about cost.

### Prompt-injection defence — has the field moved?

Yes, in two directions.

**Direction 1: layered hardening replaces "single defense."**

> "The consensus in 2026 is that no single defense eliminates prompt injection, and the right approach is layered hardening." — [TokenMix](https://tokenmix.ai/blog/prompt-injection-defense-techniques-2026)

Specifically:
- Structured input handling (delimit untrusted regions) — what our `trust:` discriminator already does.
- LLM-based filtering preprocessor (PromptArmor pattern).
- **Retrieval-gated tools**: "certain tools (anything that writes, transfers, or escalates) cannot be invoked in a turn that consumed retrieved content unless an additional confirmation step occurs." — exactly the shape of Meta's Agents Rule of Two (already in `llm-tool-design-principles.md`).
- Behavioral monitoring: anomalous tool-call patterns.

**Direction 2: visual-prompt-injection (VPI) is unsolved for CUAs.**

[VPI-Bench (Cao et al., 2025/2026, arXiv 2506.02456)](https://arxiv.org/abs/2506.02456):
- 306 test cases across Amazon, Booking, BBC, Messenger, Email.
- CUA attack success rates **up to 51 %**; browser-use agents up to **100 %**.
- "Existing defense methods offer only limited improvements" — system-prompt-only defences "largely ineffective."

Implication: our `trust:` field is necessary but insufficient. **A pixels-mode-only injection (a screenshot whose pixel content includes instructions) bypasses any string-level trust marker**, because the screenshot is a single opaque attachment.

This isn't a new problem — Anthropic's classifier defence covers it server-side — but our spec doesn't have an explicit response. Worth a follow-on ADR: "screenshots returned from `screen.capture` are always treated as `trust: external` content; the host is expected to apply visual-injection mitigation."

### Long-running computer-use postmortems

I found no public postmortem from someone who ran a multi-day computer-use agent against a real personal desktop and reported failure modes. The closest signals:
- [Anthropic — Effective harnesses for long-running agents (Nov 26 2025)](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) recommends a `claude-progress.txt`-style state file and end-to-end browser verification at session start, but is framed for coding agents.
- The current Anthropic computer-use doc adds a tip explicitly: *"For agents that span multiple sessions, run end-to-end verification at the start of each session, not only after implementation. Browser-based checks catch regressions from prior sessions that code-level review alone misses."*

For a kde-mcp MVP this maps to: at session-start, expose a cheap `session.health` tool that returns whether the EIS fd, AT-SPI bus, and ScreenShot2 service are all reachable. The agent runs it before any real work.

---

## Bucket 3 — Computer-use / desktop-control recent work

### Anthropic computer-use updates

Source: [platform.claude.com — Computer use tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool).

Since `computer_20251124` (Nov 24 2025), the action vocabulary is **unchanged**:

- All `Action_20251124` actions: `key`, `type`, `mouse_move`, `left_click`, `left_click_drag`, `right_click`, `middle_click`, `double_click`, `triple_click`, `screenshot`, `cursor_position`, `left_mouse_down`, `left_mouse_up`, `scroll`, `hold_key`, `wait`, `zoom`.
- `enable_zoom: true` opt-in for `zoom` action.
- Opus 4.7 supports up to **2576 px** on the long edge, 1:1 coords (no scale-factor conversion). Earlier models still 1568 px / 1.15 Mpx.
- The Anthropic-server-side **prompt-injection classifier** still runs on prompts with computer-use tools; can be turned off via support.

No new tool types beyond `computer / bash / text_editor`. No new actions since `zoom`. Reference impl architecture (Xvfb + mutter + xdotool + gnome-screenshot) hasn't moved; the doc page now adds an explicit tip about session-start verification (from the Long-running harness post).

The kde-mcp spec already maps Anthropic's action surface; **no spec change required** here.

### New entrants in Linux Wayland desktop automation

#### isac322/kwin-mcp v0.7.0 (Mar 29 2026) — the prior-art landscape shifted

From [the v0.7.0 release notes](https://github.com/isac322/kwin-mcp/releases/tag/v0.7.0):

> v0.7.0 transforms the project from a virtual-only testing tool into a **dual-mode desktop automation platform** — now supporting both isolated virtual sessions and live desktop connections.

Key changes:
- New `session_connect` MCP tool to attach to an existing KWin session (real desktop or container) instead of creating a virtual one. Reads `$DBUS_SESSION_BUS_ADDRESS` and `$WAYLAND_DISPLAY` from env by default.
- New `--default-live-session` server/CLI flag — make `session_connect` the default mode.
- All 30 MCP tools work identically in both virtual and live modes.
- Container support: connect to KWin running inside `systemd-nspawn` or other containers via explicit `dbus_address` and `wayland_display`.
- `session_stop` on a live session **only disconnects, never kills KWin or pre-existing applications** — a clean poka-yoke for the lifecycle issue.
- Clipboard always enabled for live sessions (no `enable_clipboard` toggle needed).

Earlier deltas worth mentioning:
- v0.6.0 (Feb 25 2026) added home-directory isolation (`isolate_home`, `keep_home`), AT-SPI2 state-aware queries (`states` filter, `wait_for_element`'s `expected_states`), role-based tree filtering, and `_run_atspi` retry-once with 500 ms backoff.
- v0.5.0 introduced `AutomationEngine` — a thin MCP-independent core, with an interactive `kwin-mcp-cli` REPL.

Open issues we should track in our `eis` design:
- [#29 libei 1.5.0 segfault in `session_connect` on Fedora 43 / Bazzite](https://github.com/isac322/kwin-mcp/issues/29) — **still open as of 2026-05-09**.
- [#8 AT-SPI accessibility tree fails in isolated sessions on Fedora](https://github.com/isac322/kwin-mcp/issues/8) — still open, last updated Apr 4.
- [#14 D-Bus auto-activation for AT-SPI bus in isolated sessions](https://github.com/isac322/kwin-mcp/issues/14) — still open, last updated Apr 21.

These are concrete reference data for our Phase 2 (EIS bootstrap) and Phase 1 (AT-SPI bootstrap) plan stages. We should test against Fedora and Bazzite explicitly.

No other public Linux Wayland desktop-control MCP server has emerged since the spec was written.

### KDE Plasma 6.5 / 6.6 surface changes

#### Plasma 6.5 (Oct 2025)

Source: [kde.org — Plasma 6.5](https://kde.org/announcements/plasma/6/6.5.0/).

Material:
- **Application Permissions page** (formerly Flatpak Permissions): users can configure per-app screenshots and remote-control acceptance. Means a kde-mcp user can grant the server's underlying flatpaked process (if we ever ship as Flatpak) explicit permission for screenshots and remote control without portal dialogs every time. For a non-flatpaked install, unchanged.
- RDP server now supports clipboard sharing; system user accounts work directly with RDP.
- Spectacle now includes pop-ups from the target window in window screen recordings.
- Photosensitivity audit completed.
- Hibernation option added to login screen.

#### Plasma 6.6 (Feb 17 2026)

Sources: [kde.org — Plasma 6.6](https://kde.org/announcements/plasma/6/6.6.0/), [itsfoss.com](https://itsfoss.com/news/kde-plasma-6-6-release/).

Material:
- **Spectacle now does OCR via Tesseract** — exposes preferences to select OCR languages. This is the most directly relevant Plasma-side change: if our `text.extract` tool needs OCR fallback (when AT-SPI has no string), Spectacle's D-Bus interface (`org.kde.Spectacle`) now exposes the OCR runtime. No need to wrap Tesseract ourselves.
- New on-screen keyboard (Plasma Keyboard, based on Qt Virtual Keyboard) for touch.
- New grayscale colorblindness filter.
- Slow Keys support on Wayland.
- Reduced Motion accessibility setting.
- Zoom/magnifier "keep pointer centred" mode.

I found **no** new public D-Bus interfaces for `org.kde.KWin.ScreenShot2 v2`, `org.kde.KGlobalAccel` changes, or AT-SPI tree changes that affect our spec. The Application Permissions page is the closest thing to a new surface; OCR-in-Spectacle is a new external capability we can call into instead of vendoring.

---

## 5. Recommendations for the plan

A numbered list of concrete plan adjustments. Each passes a cost-benefit check for a personal-use MVP — speculative additions are deferred to v2 with rationale.

1. **Update the spec citation from `MCP 2025-06-18` → `MCP 2025-11-25` before `writing-plans` runs.** Concrete touch points: §1 of the spec, §6 tool annotations (confirm naming guidance), §8 errors (map ToolError to MCP Tool Execution Errors per the changelog). Cheap. Source: [MCP changelog](https://modelcontextprotocol.info/specification/2025-11-25/changelog/).
2. **Add `examples: [...]` to every tool's annotation in Phase 1.** Anthropic measured 72 % → 90 % parameter-accuracy gain. Trivial cost — two or three call-shapes per tool — biggest single accuracy win available to a server author. Source: [Anthropic — Advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use).
3. **Pin `eis` integration testing to Fedora 43 / Bazzite specifically** to catch the libei 1.5.0 segfault upstream of our own code. Add a test matrix entry in the plan. Source: [kwin-mcp #29](https://github.com/isac322/kwin-mcp/issues/29).
4. **Borrow kwin-mcp's `_run_atspi` retry-once + 500 ms backoff pattern** in our `a11y` module. v0.6.0 release notes call this out as "improving resilience against transient AT-SPI2 bus instability" — concrete enough to copy verbatim. Source: [kwin-mcp v0.6.0 notes](https://github.com/isac322/kwin-mcp/releases/tag/v0.6.0).
5. **Adopt kwin-mcp's lifecycle invariant**: a `session_stop` on a live (real-desktop) session must only disconnect, never kill the user's KWin or pre-existing apps. Bake this into `session.close` semantics in Phase 1. Source: [kwin-mcp v0.7.0 notes](https://github.com/isac322/kwin-mcp/releases/tag/v0.7.0).
6. **Add a `session.health` cheap-check tool** that returns booleans for `eis_fd_ok / atspi_bus_ok / screenshot2_ok / clipboard_ok`. Anthropic now explicitly recommends end-to-end verification at session start for long-running agents. ~30 lines, high agent-value. Source: [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) and the related tip in [computer-use docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool).
7. **Plan Phase 3 (policy gate) to be `anthropic-experimental/sandbox-runtime` compatible.** Specifically: the policy.toml destination must default to a path *inside* the sandbox-allowed write set (e.g. `$XDG_STATE_HOME/kde-mcp/audit.log`, not `~/.config/kde-mcp/audit.log`). Audit-log writes must not require network. Source: [sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime) via [Beyond permission prompts](https://claude.com/blog/beyond-permission-prompts-making-claude-code-more-secure-and-autonomous).
8. **Write ADR-0008 (or whatever the next number is) for visual prompt injection.** Decision: `screen.capture`'s returned image is always treated as `trust: external` content; the spec recommends the host run its own VPI mitigation. This is a *documented limit*, not a code change — Anthropic's server-side classifier covers Claude users; non-Claude hosts are on their own. Don't try to solve VPI in the server. Source: [VPI-Bench](https://arxiv.org/abs/2506.02456).
9. **Add a `text.ocr` tool that delegates to Spectacle's OCR D-Bus interface** in a deferred phase (Phase 6 — Optional). Plasma 6.6 ships Tesseract-backed OCR via Spectacle; piggybacking is cheaper than vendoring Tesseract ourselves. Source: [Plasma 6.6 announcement](https://kde.org/announcements/plasma/6/6.6.0/). **Skip for MVP** unless OCR is needed.
10. **Defer Tasks-primitive adoption to v2.** It's experimental in the MCP spec; for a 15-tool personal-use server with no genuinely long actions (>10 s), the current "return when done" pattern is fine. Worth revisiting once `eis.replay_macro` and `app.launch_and_wait` exist. Source: [Tasks (experimental)](https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/).
11. **Do not adopt programmatic tool calling or tool-search-tool patterns** as server design changes — they're client-side. But: write the response shapes assuming the client *might* be using them, i.e. keep returned data structured (JSON-Schema-shaped) so a programmatic-tool-calling script can `dict["foo"]["bar"]` against it without re-parsing prose. Already the spec's §4 §1 default; just confirm.
12. **Mirror Playwright-MCP's renaming convention for safety changes**: if a tool's safety profile ever changes mid-version (e.g. an `eis.replay_macro` that was destructive-but-confirmed becomes destructive-no-confirm), rename it with an `_unsafe` suffix rather than changing semantics under the same name. Document this as a project convention in the plan, not the spec. Source: [playwright-mcp v0.0.72](https://github.com/microsoft/playwright-mcp/releases) (`browser_run_code` → `browser_run_code_unsafe`).

---

## Sources

Anthropic Engineering:
- [Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) (2025-09-29)
- [Writing effective tools for agents — with agents](https://www.anthropic.com/engineering/writing-tools-for-agents) (2025-09-11)
- [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) (2025-11-04)
- [Introducing advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use) (2025-11-24)
- [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) (2025-11-26)
- [Equipping agents for the real world with Agent Skills](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills) (2025-10-16)
- [Beyond permission prompts](https://claude.com/blog/beyond-permission-prompts-making-claude-code-more-secure-and-autonomous) (2025-10-20)

Anthropic Platform Docs:
- [Computer use tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)

MCP:
- [MCP Spec 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [MCP Changelog](https://modelcontextprotocol.info/specification/2025-11-25/changelog/)
- [One Year of MCP blog post](https://blog.modelcontextprotocol.io/posts/2025-11-25-first-mcp-anniversary/)
- [2026 MCP Roadmap](https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/)

Prior art / ecosystem:
- [microsoft/playwright-mcp releases](https://github.com/microsoft/playwright-mcp/releases)
- [isac322/kwin-mcp v0.7.0 release](https://github.com/isac322/kwin-mcp/releases/tag/v0.7.0)
- [isac322/kwin-mcp v0.6.0 release](https://github.com/isac322/kwin-mcp/releases/tag/v0.6.0)
- [kwin-mcp issue #29](https://github.com/isac322/kwin-mcp/issues/29)
- [kwin-mcp issue #8](https://github.com/isac322/kwin-mcp/issues/8)
- [kwin-mcp issue #14](https://github.com/isac322/kwin-mcp/issues/14)
- [Playwright MCP changes the build vs. buy equation (bug0.com)](https://bug0.com/blog/playwright-mcp-changes-ai-testing-2026)

Security / prompt injection:
- [VPI-Bench paper (arXiv 2506.02456)](https://arxiv.org/abs/2506.02456)
- [Microsoft Security Blog — When prompts become shells](https://www.microsoft.com/en-us/security/blog/2026/05/07/prompts-become-shells-rce-vulnerabilities-ai-agent-frameworks/)
- [TokenMix — Prompt Injection Defense 2026](https://tokenmix.ai/blog/prompt-injection-defense-techniques-2026)
- [esokia.com — Sandboxing Claude Code CLI on Linux](https://labs.esokia.com/post/sandboxing-claude-code-cli-linux-bubblewrap/)

KDE:
- [Plasma 6.5 announcement](https://kde.org/announcements/plasma/6/6.5.0/)
- [Plasma 6.6 announcement](https://kde.org/announcements/plasma/6/6.6.0/)
- [itsfoss — Plasma 6.6 OCR coverage](https://itsfoss.com/news/kde-plasma-6-6-release/)
