# kde-mcp vs Anthropic Claude Code `computer-use` — Feature/Architecture Comparison

A side-by-side comparison ignoring OS portability (treating both as if they ran on the same Linux machine), focused purely on architecture, tool surface, safety model, and agent-facing design.

**Sibling products.** Both are MCP servers that let an agent control a desktop. Anthropic's `computer-use` ships built into Claude Code on macOS (research preview, Pro/Max plans), enabled via `/mcp`. kde-mcp is the Linux/KDE equivalent.

**TL;DR:** After adopting the safety architecture documented in ADR-0007, the two are at rough feature parity. kde-mcp wins on agent-facing tool surface (a11y-first, response-diffs, ref-based poka-yoke, virtual sandbox). Anthropic wins on three things we cannot reproduce: server-side prompt-injection classifier, host-level "apps hidden during turn" feature, bundled-with-host install.

---

## What both have (after the ADR-0007 upgrade)

These are tied or substantively equivalent:

- **Single-session lock with KDE/macOS notifications.** Acquire-on-start, release-on-exit, stale-lock detection. We split into `live.lock` + `virtual.lock`; theirs is machine-wide singleton.
- **Per-app approval per session.** First destructive action against an unapproved app → user prompt with Allow/Deny actions. Theirs is terminal-prompt; ours is `org.freedesktop.Notifications` D-Bus prompt.
- **App-category tiering by risk.** `view_only` (browsers), `click_only` (terminals + IDEs), `full` (default). Same three tiers; same intent.
- **Sentinel warnings.** Apps with broad reach (terminals, file managers, system settings) flagged in approval prompts and response `warnings`.
- **Terminal excluded from screenshots.** Both prevent on-screen content in the agent's terminal from feeding back as prompt injection. Theirs hides the terminal window from capture; ours masks the terminal's bbox with a black rectangle.
- **Global cancel shortcut, key consumed.** Theirs: Esc. Ours: `Meta+Esc` (config-overridable). Both grab at compositor level so apps don't see the key.
- **MCP-as-protocol.** Standard MCP over stdio; works with any compliant host.
- **Screenshot downscaling.** Theirs auto-downscales (e.g. 3456×2234 → ~1372×887). Ours configurable via `max_long_edge`, default 1280.
- **Bash and text-editor semantics.** Essentially identical surface.

---

## Where kde-mcp wins

These are kde-mcp's structural advantages that Anthropic's design does not have. They are why this project exists even on an OS-agnostic basis.

### 1. Accessibility-tree-first as the primary surface

Anthropic's tool surface (as documented) is **pixel-and-screenshot first**: "click, type, see your screen." The macOS Accessibility API exists and is presumably used for *executing* clicks/typing, but there is no documented `act_on_element(ref, action)` primitive that does AT-SPI-style `do_action()` invocations as the agent's primary reasoning surface.

kde-mcp makes `act_on_element` the primary primitive. `mouse`/`screenshot` are explicit fallbacks marked `vision_fallback: true`. For native Qt/GTK4 apps this is **deterministic** — no coordinate hallucination, no missed clicks. (ADR-0005)

### 2. Response shape carries decision-relevant deltas

Every action returns a structured `diff` (`windows_added`, `focused_element`, `cursor`) so the agent sees side effects without a follow-up screenshot. Anthropic's tool returns a screenshot after every action — the agent decodes pixels to know what changed. (ADR-0004)

### 3. Structural trust-marking on every external string

Window titles, AT-SPI labels, clipboard reads, OCR text — every model-readable string the MCP returns carries `trust: "external"`. The model sees data marked as data, not as instructions. Anthropic relies on the server-side classifier; we add a structural defense the agent can read in-context. (Spec §10 layer 8, post-ADR-0013)

### 4. Token economy

No screenshot after every action. No 700–1000 tokens per response. On a 10-action sequence we measure 4–5× cheaper than Anthropic's reference impl. (Spec §4.3)

### 5. Coordinate poka-yoke

`act_on_element` accepts opaque refs only; the schema doesn't permit `(x, y)`. The model **cannot pass a coordinate where a ref is required** — the wrong action is structurally impossible to express. (Spec §4.5)

### 6. Sandboxed virtual mode

`kde-mcp --mode=virtual` spawns an isolated `kwin_wayland --virtual` session for unattended automation. Anthropic's `computer-use` is live-only — it operates on your real desktop, period.

### 7. Programmatic primitives Anthropic doesn't have

- `window_query` — list/find windows by criteria, no screenshot needed
- `window_control` — focus/move/resize by ref
- `inspect` — paginated a11y-tree drilldown
- `clipboard_read/write` — first-class clipboard with origin info
- `act_on_element` — direct AT-SPI action invocation against a ref (no input emulation needed)

Anthropic's `computer-use` also ships `bash` and `text_editor` — we explicitly **do not**. ADR-0013 records the rule: an MCP tool earns its place only when the host can't already run the equivalent. Claude Code's native bash and Read/Edit/Write cover those domains; duplicating them in this MCP would create parallel state (two shells, two file-edit audit trails) without adding capability.

### 8. Auditable and extensible

JSONL audit log (`~/.local/state/kde-mcp/audit.jsonl`) structured for `tail -f` review. Adding a new capability is a new module under `src/tools/`. Anthropic's tool surface is whatever they shipped; you can't extend it.

---

## Where Anthropic wins — features we deliberately don't reproduce

These three are documented in ADR-0007 with substitution rationale.

### ❌ 1. Server-side prompt-injection classifier

Anthropic runs a classifier on prompts containing computer-use tools and flags potential injection from on-screen content. We have no classifier weights, no training data, no inference budget.

**Substitute:** Trust wrappers (spec §10 layer 10) + sensitive-path warnings (§10 layer 9) + per-app approval (§10 layer 4) which adds a human-in-the-loop checkpoint for high-risk actions.

### ❌ 2. Hidden non-approved apps while Claude works

Anthropic auto-hides all non-approved apps during a turn so Claude only sees what it was approved for. This requires host-level turn awareness we don't have as an external MCP server.

**Substitute:** Per-app approval (§10 layer 4) + tier restriction (§10 layer 5). The agent can still *see* other apps but cannot *touch* unapproved ones. Different safety property; arguably weaker for unattended automation, acceptable for personal use.

### ❌ 3. Bundled-with-host install

Anthropic ships `computer-use` inside the Claude Code binary; users enable via `/mcp`. We're a separate binary that has to be installed.

**Substitute (post-MVP):** `.mcpb` extension bundle for Claude Desktop + `claude plugin install kde-mcp` for Claude Code. One-command install is the closest reasonable parity.

---

## Side-by-side feature matrix

| Dimension | Anthropic `computer-use` | kde-mcp |
|---|---|---|
| **MCP protocol** | ✅ | ✅ |
| **Single-session lock + notifications** | ✅ machine-wide | ✅ per-mode |
| **Per-app approval per session** | ✅ terminal prompt | ✅ KDE notification with action buttons |
| **App-category tiering (view/click/full)** | ✅ | ✅ |
| **Sentinel warnings** | ✅ | ✅ |
| **Terminal excluded from screenshots** | ✅ window-hidden | ✅ bbox-masked |
| **Global cancel shortcut, key consumed** | ✅ Esc | ✅ Meta+Esc (configurable) |
| **Auto-downscaling screenshots** | ✅ fixed | ✅ configurable |
| **Audit log** | (not documented) | ✅ JSONL local |
| **A11y-tree as primary action surface** | ❌ pixel-first | ✅ `act_on_element` |
| **Response-diff after each action** | ❌ screenshot-after-each | ✅ structured diff |
| **Coordinate poka-yoke** | ❌ coords accepted everywhere | ✅ refs required |
| **Structural trust-marking** | ❌ classifier substitutes | ✅ `trust: external` wrappers |
| **Sandboxed virtual mode** | ❌ live only | ✅ `--mode=virtual` |
| **Programmatic window query/control** | ❌ vision-only | ✅ first-class |
| **Server-side prompt-injection classifier** | ✅ | ❌ structural substitutes |
| **Hidden non-approved apps during turn** | ✅ | ❌ tier + approval substitute |
| **Bundled-with-host install** | ✅ | ❌ separate install (.mcpb planned) |
| **`bash` / `text_editor` parity** | ✅ included | ❌ deliberately excluded (ADR-0013 — host owns shell + file edits) |
| **Tool count** | not documented (estimated ~5) | 11 logical / 12 wire (ADR-0013) |

---

## Honest read

**The two are siblings, not competitors**, and each has clear strengths.

**Anthropic's strength** is the host integration depth — per-app approval that's built into the terminal flow, app hiding, server-side classifier, bundled install. These are *product engineering features* that come from controlling the host. After ADR-0007 we replicate most of the user-visible safety surface, but their server-side classifier and host-level app hiding are genuinely uncompetable.

**kde-mcp's strength** is the agent-facing tool surface — accessibility-tree primacy, response-diffs, trust-wrappers, ref-based poka-yoke, structured waits, virtual sandbox, extensible architecture. These are *agent engineering features* that come from optimising for the model's attention budget rather than the user's UI flow.

**On the same Linux machine, given identical safety polish (post-ADR-0007), kde-mcp is the better agent-facing design** for the structural reasons above. The 4–5× token reduction on multi-step workflows, the deterministic AT-SPI invocations, and the sandboxed virtual mode are real, measurable, OS-independent advantages.

**On the same Linux machine, ignoring everything else and looking only at "is the safety story good enough,"** Anthropic's design is mature and ours (post-ADR-0007) is close but not identical. Specifically: our human-in-the-loop checkpoint is at the approval-prompt stage; theirs additionally hides apps during a turn so the agent can't even see un-approved apps. For personal-use single-user it doesn't matter; for shared-machine or remote-driven scenarios, it does.

## When to **not** use kde-mcp

- You need cross-platform (macOS / Windows) — use Anthropic's bundled solution there.
- You need a server-side classifier defense baseline — use Anthropic's.
- You need "agent literally cannot see un-approved apps" semantics — use Anthropic's host-level model.

For everything else on a KDE Plasma 6 Wayland desktop, kde-mcp is the more agent-friendly design.
