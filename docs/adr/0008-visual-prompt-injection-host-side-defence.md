# ADR-0008: Visual prompt injection defence is host-side, not server-side

**Status:** Accepted
**Date:** 2026-05-14
**Deciders:** atassis

## Context

The spec's quarantine model (§4.7) wraps every external string in a `trust` discriminator (`trusted | agent_authored | external`) so the model and downstream tools treat agent-readable text as data, not instruction. ADR-0007 explicitly chose to skip the server-side prompt-injection classifier Anthropic ships, on the grounds that "we have no classifier weights, no training data, no inference budget" — substituting structural defences (trust wrappers, per-app approval, sentinel warnings, terminal-excluded screenshots).

Research preceding the implementation plan surfaced a gap that none of those defences cover: **visual prompt injection (VPI)**. The 2025 VPI-Bench paper measures computer-use agents (CUAs) under pixel-content attacks — adversarial UI text, instructional overlays inside images, deceptive labels rendered into screenshots — and reports up to 51 % attack success on frontier models, with current defences offering "only limited improvements." See `docs/research/2026-05-13-mcp-ecosystem-trends.md` for the full citation.

The `trust` discriminator quarantines **strings**: AT-SPI `name`/`value`/`description`, clipboard reads, OCR text, bash stdout, file contents. It does not — and cannot — quarantine **pixels**. A `screenshot` response carries a PNG. The PNG can contain rendered text the model reads at attention time. That text is not a string the server can wrap. The same applies to `read_text` outputs after OCR (those strings *are* wrapped, but the model may have already read the unprocessed image in the same turn).

This was not called out in any existing ADR.

## Decision

Visual prompt injection defence is **host-side**, not server-side. `kde-mcp` does not attempt to detect, mask, or sanitise adversarial pixel content. The mitigation surface lives with the model host (Anthropic's classifier, the agent runtime's image-preprocessing pipeline, etc.).

Server-side, we ratify the existing scope and document the limit explicitly:

1. **Trust wrappers cover strings only.** §4.7 stands as written. Adding a `trust` field to image payloads would mislead — the field cannot influence model attention at the pixel level.
2. **The structural defences we already have are listed honestly.** Per-app approval (ADR-0007) puts a human in the loop before destructive actions, regardless of how the agent decided to act. Sentinel warnings + terminal-excluded screenshots + the `Meta+Esc` global cancel give the user observable abort surfaces. None of these *prevent* a pixel injection from being read; they bound the blast radius if one succeeds.
3. **`vision_fallback: true` markers** on responses that returned via pixels (`mouse.click` with raw coordinates, `screenshot`, `read_text`) remain. They flag for the audit log and the model's next-turn context that the previous action was pixel-driven — useful evidence if a user is investigating a suspicious turn.

## Rationale

- **Same logic as ADR-0007's "skipped classifier" rationale.** Inferring adversarial intent from arbitrary pixel content is a model-quality problem. The server has neither the inputs (attention activations, RLHF signal) nor the compute budget. Pushing it onto the host concentrates VPI defence where it is actually solvable.
- **Pretending otherwise would be worse.** A token-level `image_trust: "external"` field would create the appearance of mitigation without the mechanism. Worse: agents might rely on the field as a heuristic instead of treating *all* pixel content as untrusted by default.
- **Personal-use scope.** ADR-0002's framing still applies. The user runs against their own desktop; the threat model is "an app I opened renders text that fools the agent," not adversarial inputs at scale. For this scope, host-side classification + the structural defences listed above are proportionate.
- **The audit log is the recourse.** If a VPI-driven mistake happens, the JSONL audit (§9) records: which tool, which window class, what action, what (string-wrapped) text was read. Forensics work on the audit; prevention is layered above.

## Consequences

- **No new code or config in this server.** This ADR documents a non-decision: the absence of a server-side VPI defence is now explicit, not implicit.
- **Documentation update.** The honest-comparison doc (`docs/comparison-with-anthropic-computer-use.md`) gains a row: "Anthropic ships a server-side prompt-injection classifier that covers both text and visual prompt injection (the latter via image preprocessing in their pipeline). `kde-mcp` covers only text via the trust discriminator. Pixel-level VPI is the host's responsibility either way."
- **Future revisitation triggers.** Three things would justify revisiting this ADR:
  1. A pre-trained, locally-runnable VPI classifier ships at a cost the project can absorb.
  2. The MCP spec adds first-class image-content trust metadata that hosts agree to honour.
  3. A non-trivial fraction of audit-log incidents trace to VPI on this particular setup.
- **No phase impact.** ADR-0007's policy gate (§6.0) and the trust discriminator (§4.7) are unchanged. Phase scopes in §14 do not move.

## Alternatives considered

- **Run an OSS image classifier (CLIP, BLIP-2, or similar) over every `screenshot` response.** Rejected: latency, GPU dependency, and the published accuracy of such classifiers on adversarial VPI is poor (per the same VPI-Bench paper). Cost would dominate; benefit is unproven.
- **Mask all rendered text in screenshots with a blur or solid overlay.** Rejected: defeats the purpose of `screenshot` as a fallback when AT-SPI is empty. The agent reads pixels precisely because the text is the load-bearing content. Masking it is worse than the disease.
- **Refuse to return `screenshot` for apps not in an allowlist.** Rejected: the policy gate already enforces per-app approval for destructive actions; gating `screenshot` (read-only) on top of that would prevent observation, which is the first thing the agent does on entering an unfamiliar state (§4.2's reflex-first-call pattern).
- **Add `image_trust: "external"` field to screenshot responses.** Rejected: see Rationale above. Misleading without enforcement.

## Open questions

- **Does Anthropic's `computer_20251124` host pipeline actually handle VPI?** Their documentation doesn't specify, and the VPI-Bench numbers suggest model-level mitigation is partial at best. The honest framing is "host-side is the only place this can be addressed, and even there it's an open problem."
- **Should `read_text` (OCR) outputs gain a `vision_origin: true` field** so the agent knows the wrapped string came from a fallible pixel-recognition step? Probably yes, but it's a small response-shape addition, not an ADR-level decision. Carry into the plan as a §6.1 detail.

## Source

Recorded 2026-05-14 during the research that preceded the implementation plan. Primary citation: VPI-Bench (2025), surfaced via `docs/research/2026-05-13-mcp-ecosystem-trends.md` Bucket 2. Framed as a new ADR rather than a revision to §4.7, on the grounds that ADRs are for non-obvious decisions someone might reasonably revisit — and "should the server defend against pixel injection?" is exactly that kind of question.
