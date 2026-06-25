# ADR-0006: 15 tools split read/write; no Anthropic-style omni-tool

**Status:** Accepted (catalogue trimmed to 11 logical tools by [[0013-remove-shell-and-file-tools]] on 2026-05-16; the split-along-safety / consolidate-along-intent principles in this ADR still govern the remaining tools)
**Date:** 2026-05-13
**Deciders:** atassis

## Context

Three shapes were considered for the tool surface:

- **A. Anthropic-style omni-tool.** One MCP tool named `computer` with an `action` string parameter (`left_click`, `type`, `scroll`, `screenshot`, ...) matching `computer_20251124`'s vocabulary. **Pro:** Claude has training priors on this shape — best coordinate accuracy. Prompt portability. **Con:** less idiomatic MCP, harder per-action permissioning, opaque to non-Claude models, omits Linux-specific primitives like `accessibility_tree` and `window_control`.

- **B. Many small idiomatic MCP tools.** ~25 tools at primitive granularity (`mouse.click`, `mouse.move`, `keyboard.type`, `keyboard.key`, `window.list`, ...). **Pro:** standard MCP idiom, fine-grained permissions, friendly for non-Claude models. **Con:** more JSON-schema tokens in `tools/list`, Claude loses some training transfer.

- **C. Layered (both).** Omni `computer` tool + extra MCP tools for Linux-specific primitives. **Pro:** training transfer + extra primitives. **Con:** ~10 tools plus overlap; some duplication.

After the user's clarification on the design philosophy ("fewest tools, each as powerful as possible, prefer programmatic truth over visual interpretation"), a fourth option emerged:

- **D. ~10–12 layered tools, accessibility-tree-first, with read/write splits along safety axes.** Each tool covers an intent surface; structured truth is the fast path; pixels are explicit fallback. The user's philosophy translates directly into this shape.

## Decision

Take **D**, refined to **15 tools after read/write splits for clean permissioning**:

| Read-only (8) | Destructive (7) |
|---|---|
| `observe` | `act_on_element` |
| `inspect` | `mouse` |
| `screenshot` | `keyboard` |
| `read_text` | `window_control` |
| `wait` | `clipboard_write` |
| `window_query` | `bash` |
| `clipboard_read` | `text_editor_edit` |
| `text_editor_view` | |

The read-only set autoapproves in Claude Code's default permission mode. The destructive set triggers approval. The split costs ~7 extra tool definitions in `tools/list` but pays back every turn the agent does a read-only operation.

## Rationale

- **Anthropic's omni-tool is API-private.** MCP cannot transport `type: "computer_20251124"`. Even if we ship a `computer` tool with that name and action vocabulary, it pays full JSON Schema token cost (no training-baked shorthand) and skips Anthropic's classifier defense.
- **The "training priors" argument weakens** when our action vocabulary diverges from Anthropic's (we add `act_on_element`, `inspect`, `window_query`, `wait_for_condition`). Claude doesn't have priors on those.
- **Linux-specific primitives are too valuable to omit.** AT-SPI invocation, window queries, structured `wait` conditions, semantic clipboard — none of these fit Anthropic's vocabulary, all of them save tokens and improve reliability.
- **Read/write splits work with the host's permission model.** Claude Code's default mode autoapproves `readOnlyHint: true` tools and prompts on `destructiveHint: true`. Splitting `text_editor` into `_view` + `_edit` (and likewise for `window` and `clipboard`) gives the host clean control without us building a parallel confirmation system.
- **The 15-tool count is in the documented sweet spot** (Anthropic + PagerDuty + pgEdge converge on 20–25 as the limit; we're under, with headroom).

## Consequences

- Tools' descriptions become Linux-native, not Anthropic-portable. Existing computer-use prompts will need light editing.
- `act_on_element` is the primary action tool — needs careful documentation.
- Refs (element_id, window_uuid) must be validated server-side (the poka-yoke principle, ADR-0004).
- Adding a new capability is a tool *or* a response-field decision; the principles in the spec govern which.

## Alternatives considered

- **A (Anthropic-style omni-tool).** Rejected — loses Linux-specific primitives and the training-priors argument weakens once we diverge.
- **B (25 small primitives).** Rejected — too granular, agent spends tokens reconstructing workflows.
- **C (omni + extras).** Rejected — overlap and duplication.

## Source

The guiding philosophy: maximise functionality while minimising tool count, and make each tool as useful as it can be — and wherever a programmatic answer exists (a list of windows or screens) prefer it over a screenshot. That maps directly onto D — combined with Playwright-MCP's a11y-first philosophy and Anthropic's "consolidate along intent, split along safety" principle.
