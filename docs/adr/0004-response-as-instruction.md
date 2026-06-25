# ADR-0004: Response-as-instruction over prompt-prose

**Status:** Accepted
**Date:** 2026-05-13
**Deciders:** atassis

## Context

When designing agent-facing tools, there are two ways to influence the agent's next action:
1. Add prose to the tool *description* ("after calling this, you may want to ...")
2. Add structured fields to the tool *response* (`{ "diff": { "windows_added": [...] } }`)

Tool descriptions live thousands of tokens away in context (in the system prompt or near the tool list). Tool responses are the freshest tokens in the agent's attention window.

The agent demonstrably attends more strongly to recent tokens than to system-prompt boilerplate. A paragraph in a description saying "you may want to call observe after Ctrl+S" is unreliable. A field `windows_added: [{ref, title}]` on the response is unmissable.

## Decision

**Encode behaviour-shaping information in structured response fields, not in prose at the top of tool descriptions.** Tool descriptions become short, factual statements about what the tool does. Responses carry the deltas, hints, recovery state, and warnings that drive the next decision.

## Rationale

- **Attention proximity.** Recency bias in transformer attention is real; response tokens are seen "right now" while description tokens were seen ago.
- **Concreteness.** "Here is the new window that just appeared" is unambiguous; "you may want to wait for new windows" is advisory and easily ignored.
- **Token economy.** Descriptions are spent once per turn (in the tool list). Responses are spent only when a tool is called. If you want guidance to apply 100% of the time after a tool returns, putting it in the response costs exactly the calls that need it.
- **Self-describing growth.** New behaviours can be added by adding response fields; existing prompts continue to work. Prose-based guidance bit-rots when the surrounding paragraph changes.

## Concrete applications baked into the spec

(See §4.2 of the design doc.)

1. Action tools (`mouse`, `keyboard`, `act_on_element`, `window_control`) return a `diff` field with structured side effects.
2. `stale_handle` errors include `current_state` inline (a fresh `observe` payload).
3. `wait` timeouts include `closest_match` and `current_windows`.
4. `act_on_element` failures include `available_actions` and `fallback_suggestion`.
5. `screenshot` responses include the structural snapshot captured at the moment of capture.
6. Sensitive-path edits return `warnings: ["modified file under ~/.ssh/"]`.
7. Virtual mode with no windows returns `hint: "virtual session has no running apps; use bash to launch one"`.

## Consequences

- Tool descriptions get to be 1–3 sentences each, factual.
- Each response shape is more complex than a naive "ok / error" — adds ~80–200 tokens per action call. Cost is small relative to a screenshot (~500–1000 tokens) and changes behaviour reliably.
- Any new tool added later **must follow this** — guidance goes into response fields, not into description prose, except for irreducible factual statements about what the tool does.
- Documenting the principle here means future contributors can be told "use the response, not the description" instead of relearning the lesson.

## Alternatives considered

- **Long, didactic tool descriptions.** Standard practice in many MCP servers. Rejected — measurably unreliable and burns tokens every turn.
- **Auto-screenshot after every action.** Anthropic's reference impl pattern. Rejected — 500–1000 tokens per action even when no visual question, the model often doesn't need it.
- **A "side channel" `notifications/progress` stream.** MCP supports it but client UI support is uneven; less reliable than putting deltas in the tool result.

## Source for the principle name

The principle crystallised during design: an agent steers more reliably on information delivered *in the response it just received* than on instructions stated once, far back in a tool description. A `diff` field returned alongside the action lets the agent react to the change in the moment, rather than relying on a paragraph it read many turns ago.

Generalisation of patterns documented in:
- Anthropic — Writing tools for agents
- Anthropic — Effective harnesses for long-running agents
- Inkeep — Fighting Context Rot
