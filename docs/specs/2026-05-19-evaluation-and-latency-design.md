# Evaluation and Latency Design

**Status:** Proposed — 2026-05-19. Author: atassis.
**Supersedes:** nothing.
**Related:** ADR-0005 (accessibility-tree first), ADR-0013 (MCP-purpose rule), spec §15 (open questions).

---

## 1. Context

ADR-0005 made a structural bet: an accessibility-tree-first server with pixels as fallback will outperform a vision-only server for desktop-control workflows. The spec's §15 left this bet untested; the post-MVP memory frames it as two questions:

1. **Where do the milliseconds go in a kde-mcp tool call?** — pure introspection.
2. **Is vision-mode genuinely needed for the workflows kde-mcp targets?** — comparative.

This design splits those into two cheap, separable studies. Study 1 is a $0 instrumentation + reporting exercise. Study 2 is a small adversarial probe against the actual capability boundary, not a statistical winner-picking contest. Together they inform the project's next-investment decision (tool-description tuning vs new tools vs pixels-augmented mode) at roughly $10–18 in credit.

The design deliberately rejects the larger "run S1–S6 plus extras against both arms with statistical n" approach. S1–S6 were authored to verify the implementation against the spec; reusing them as A/B fuel double-bills them and biases the comparison toward arm A. The project decision is qualitative direction-finding, not quantitative winner-picking.

---

## 2. Goals and decision rules

### Study 1 — Latency profile

- **Question:** Where do ms go in a kde-mcp tool call, and is any single phase a problem?
- **Decision rule:** If any phase's p95 > 100 ms, file a tracked optimisation issue. If total p95 < 250 ms across all tools, the number is quotable publicly.
- **Reusable artefact:** a `latency-analyzer` package that re-runs against any audit-log corpus.

### Study 2 — Adversarial probe

- **Question:** On workloads where text-only-on-AT-SPI is known-weak (Electron content, low-hygiene Qt, drag precision, rasterised content), what classifies each failure mode?
- **Decision rule:** For each of 4 scenarios, classify the text-only outcome into one of:
  - `pass`
  - `fail-tool-use-gap` — data is exposed; Claude didn't reach for it
  - `fail-tool-design-gap` — not exposed today; a new kde-mcp tool plausibly could
  - `fail-essential` — pixels/OCR is the only path; no accessibility surface helps
- Then:
  - 3–4 essential → vision matters; design a pixels-augmented mode
  - 3–4 tool-design-gap → invest in new tools (journey tools, richer observe)
  - 3–4 tool-use-gap → invest in tool descriptions / schema clarity
  - Mixed → publish the case studies; revisit in 3 months
- Scope of inference: this is a capability-boundary probe, not an average-workflow benchmark. The user infers from "vision wins on X" to "my workflows hit X."

---

## 3. Study 1 — Latency profile

### 3.1 Architecture

Two pieces, separated along the producer/consumer line.

**Producer (in `kde-mcp`, Rust).** Audit `src/audit.rs` for per-tool latency fields. Where missing, add five timestamps wrapped around each tool call in `src/server.rs::dispatch_audited`:

- `t_received` — request hit rmcp deserialiser
- `t_policy_done` — policy gate returned
- `t_impl_start` / `t_impl_done` — wrapping the tool body
- `t_audit_written` — after the audit record flushes

Total wall-clock and four phase durations get serialised into the existing JSONL record as optional fields. No new files, no new CLI subcommand in the Rust binary, no schema break.

**Consumer (new, TypeScript).** A small Node package at `tools/latency-analyzer/` in the repo:

```
tools/latency-analyzer/
├── package.json
├── tsconfig.json
├── src/
│   ├── parse.ts        # jsonl streaming parser
│   ├── aggregate.ts    # histogram/percentile math
│   ├── report.ts       # markdown emitter
│   └── cli.ts          # argv handling
└── README.md
```

Runtime: Node 22+ or Bun. Plain TypeScript, no framework. The choice of TypeScript matches the natural ergonomics of jsonl streaming + markdown emission and keeps the Rust daemon focused per ADR-0013's MCP-purpose rule.

### 3.2 Usage

```bash
cd tools/latency-analyzer
bun run report --since 7d > docs/latency-profile.md
```

### 3.3 Output format

```markdown
# kde-mcp latency profile — 7 days ending 2026-MM-DD

N = 1,247 tool calls across 12 tools.

| Tool        | n   | p50 ms | p95 ms | p99 ms | dominant phase (p95)        |
|-------------|----:|-------:|-------:|-------:|-----------------------------|
| observe     | 312 |    45  |    98  |   140  | tool_impl (AT-SPI walk)     |
| screenshot  |  87 |   220  |   480  |   720  | tool_impl (ScreenShot2)     |
| ...         | ... |   ...  |   ...  |   ...  | ...                         |

## Phase-level breakdowns

[Per-tool: stacked p50/p95/p99 of each phase, in ms]
```

### 3.4 Operational

- Run during a normal week of kde-mcp use; no synthetic load.
- Re-generate after any commit that touches `src/server.rs`, `src/tools/*`, or `src/audit.rs`.
- The analyzer reads `~/.local/state/kde-mcp/audit.jsonl` (path configurable via flag).

---

## 4. Study 2 — Adversarial probe

### 4.1 Scope of inference

This is a **capability-boundary probe**, not an average-workflow benchmark. Each scenario isolates one theoretical weakness of text-only-on-AT-SPI; the goal is to classify *where* and *why* each arm breaks, not to declare a winner across average use. A reader who wants to project these findings to their own workflows must check whether those workflows hit the same boundaries.

n=2 per (scenario × arm), tiebreaker n=3 on pass/fail disagreement. ~16–20 runs total.

### 4.2 Scenarios

Each scenario is a fixture under `harness/fixtures/` plus a task prompt. All four have programmatic verification and are reproducible from a clean state.

| Code | Fixture | Task | Probes | Verification |
|---|---|---|---|---|
| **E** | `probe-canvas.html` — 5 buttons rendered as `<canvas>` showing digits 1–5 (no DOM text). Open in Brave. | "Click the canvas button showing the number 3." | AT-SPI exposure gap on canvas content. DOM/AT-SPI sees 5 indistinguishable `<canvas>` nodes; vision reads digits. | Page emits clicked-index to `/tmp/probe-e.txt`. Expect `3`. |
| **Q** | `probe-qt-noname.py` — PySide6 window, 3 buttons with display text "Alpha/Beta/Gamma" but no `accessibleName`. | "Click the Beta button." | App-side AT-SPI hygiene gap. Names readable visually, invisible to AT-SPI. Text-only must fall back to `screenshot` + `read_text` (OCR). | Click handler writes button index. Expect `1`. |
| **D** | `probe-drag.py` — PySide6 QGraphicsView with a 20×20 handle at (100, 100). | "Drag the handle 60 pixels to the right." | Coordinate precision. Both arms ultimately use `mouse(drag)` with coords; question is whether vision converges faster. | Final handle x ∈ [155, 165]. |
| **R** | `probe-chart.png` — pre-rendered bar chart, 4 bars labeled "12 / 73 / 45 / 31". Open in Gwenview. | "What's the value of the tallest bar?" | Rasterised-only content. AT-SPI has nothing; text-only must call `read_text` (Tesseract OCR). | Final agent response contains the substring `73`. |

### 4.3 Harness architecture

```
harness/
├── pyproject.toml          # anthropic, httpx, pyyaml, PySide6
├── fixtures/               # the 4 fixtures from §4.2
├── scenarios/              # E.yaml, Q.yaml, D.yaml, R.yaml
├── src/harness/
│   ├── cli.py              # `run-all`, `run --scenario X --arm Y`, `report`, `smoke`
│   ├── loop.py             # shared agentic loop, arm-agnostic
│   ├── mcp_client.py       # httpx wrapper for kde-mcp HTTP at 127.0.0.1:7575
│   ├── arms/
│   │   ├── kde_mcp_arm.py  # arm A
│   │   └── vision_arm.py   # arm B
│   ├── scenario.py         # load YAML, setup, verify, teardown
│   ├── logger.py           # one .jsonl per run
│   └── smoke.py            # phase-0 smoke test
└── runs/<timestamp>/       # gitignored
    ├── E-kde-mcp-1.jsonl
    ├── E-vision-1.jsonl
    ├── ...
    └── summary.md
```

### 4.4 Shared loop

```python
def loop(scenario, arm):
    scenario.setup()                      # launch fixture, focus, capture starting geometry
    try:
        messages = [{"role": "user", "content": scenario.prompt}]
        spend = 0.0
        for step in range(STEP_RUNAWAY_CAP):   # 50, safety only
            resp = client.beta.messages.create(
                model="claude-opus-4-7",
                tools=arm.tool_definitions(),
                betas=arm.beta_headers,
                messages=messages,
                stream=False,
            )
            log_step(step, resp)
            spend += cost_of(resp.usage)
            if spend > BUDGET_CAP_USD:          # $0.50, guardrail
                return "timeout-budget"
            tool_results = arm.execute_tools(resp)
            if not tool_results:
                # Claude stopped requesting tools — task complete or gave up
                return classify(scenario.verify())  # pass | fail-* 
            messages.append(...)
        # Fell off the for-loop without `break`: ran out of steps
        return "timeout-steps"
    finally:
        scenario.teardown()
```

`classify()` maps `verify()`'s pass/fail boolean plus the run's log into one of the §4.10 fail categories. The classification logic is the diagnosis protocol (§4.11), not encoded as automation here.

### 4.5 Arm A — kde-mcp surface

- `tool_definitions()`: at startup, GET kde-mcp's `tools/list`; translate MCP schemas → Anthropic tool format. All 12 wire tools exposed.
- `execute_tools(resp)`: each `tool_use` block becomes an MCP `tools/call` POST to `http://127.0.0.1:7575/mcp`. Result fed back to Claude as `tool_result`.
- `beta_headers`: `[]`.

### 4.6 Arm B — computer-use surface

- `tool_definitions()`: exactly one tool:
  ```json
  {"type": "computer_20251124", "name": "computer",
   "display_width_px": <screen W>, "display_height_px": <screen H>}
  ```
- `beta_headers`: `["computer-use-2025-11-24"]`.
- **Nothing else exposed.** Claude sees only the computer tool.
- `execute_tools(resp)` translates Anthropic computer-use actions → kde-mcp daemon calls. Same Wayland primitives both arms — that property is deliberate (no reference computer-use exists for KDE Wayland; any vision impl must use the same KWin/libei/ScreenShot2 primitives anyway). Differential outcomes are therefore genuinely tool-surface + LLM-conditioning, not Wayland-library choice.

**Bridge action coverage** (in `harness/arms/vision_arm.py` as a constant). Verified against `src/tools/mouse.rs` (actions: click, move, drag, scroll, down, up, hold) and `src/tools/keyboard.rs` (`keyboard_type` and `keyboard_key` with optional `hold_ms`).

| Anthropic action          | kde-mcp call                                    | Implemented? |
|---------------------------|-------------------------------------------------|--------------|
| `screenshot`              | `screenshot(max_long_edge=full)`                | yes |
| `left_click`              | `mouse(action=click, button=left)`              | yes |
| `right_click`             | `mouse(action=click, button=right)`             | yes |
| `middle_click`            | `mouse(action=click, button=middle)`            | yes |
| `double_click`            | `mouse(action=click, count=2)`                  | yes |
| `triple_click`            | `mouse(action=click, count=3)`                  | yes |
| `mouse_move`              | `mouse(action=move)`                            | yes |
| `left_click_drag`         | `mouse(action=drag)`                            | yes |
| `left_mouse_down`         | `mouse(action=down, button=left)`               | yes |
| `left_mouse_up`           | `mouse(action=up, button=left)`                 | yes |
| `scroll`                  | `mouse(action=scroll)`                          | yes |
| `type`                    | `keyboard_type(text=...)`                       | yes |
| `key`                     | `keyboard_key(keys=...)`                        | yes |
| `hold_key`                | `keyboard_key(keys=..., hold_ms=duration*1000)` | yes |
| `wait`                    | `time.sleep` host-side                          | yes |
| `zoom`                    | crop kde-mcp screenshot host-side               | yes |

All current Anthropic computer-use actions map to existing kde-mcp tools. The `bridge_gap` event type and tool-call-arm exist as a **forward-compat safeguard** — if Anthropic adds new actions in a future beta, or if the harness encounters an action variant not handled (e.g., a modifier-key combination the bridge translation didn't anticipate), the action returns `{"is_error": true, "content": "bridge-gap: <action> not implemented"}` and the harness logs `bridge_gap`. With today's coverage this should not fire; if it does, that is itself a finding.

### 4.7 Phase 0 smoke test

Before authoring the rest, validate that the `computer_20251124` tool is granted to the user's account. `harness/smoke.py` is a 50-line standalone script:

1. Load `ANTHROPIC_API_KEY` from env; fail loudly if absent.
2. Call `client.beta.messages.create(...)` with the `computer` tool, beta header, and one user message ("take a screenshot").
3. If Claude requests `screenshot`, return a stub 1×1 transparent PNG as the `tool_result` content (the smoke test does not need a real screenshot — the goal is just to confirm the beta header is honoured and the tool_use round-trip works).
4. Confirm a `tool_use` block was issued. Print PASS or the specific failure mode (permission_denied / model_not_granted / other).

Cost: ~$0.01. **Gates Phase 1.** Failure modes and responses:

- `permission_denied` / `model not granted` → the account/plan doesn't (yet) include this beta. Surface to user; do not proceed.
- Network / transient API error → retry once; if still failing, surface; do not proceed.
- Returns text instead of `tool_use` → unexpected; capture full response and surface.

The rest of the work does not start until smoke is green. Do not work around a red smoke by stubbing the vision arm.

### 4.8 Run budget and caps

- **Cost cap:** `BUDGET_CAP_USD = 0.50` per `(scenario × arm × trial)`. Treated as **guardrail**; classification `timeout-budget` is reported as a finding, **not** weighed in pass/fail tally.
- **Step cap:** `STEP_RUNAWAY_CAP = 50`. Safety only; should never fire.
- **Trials:** `n=2` per `(scenario × arm)`. If trials disagree on pass/fail, `n=3` tiebreak. The failure-mode itself is reported per-trial (no majority on failure-mode).
- **Total credit projection:** ~$10–18.

### 4.9 Logging schema

One JSONL file per `(scenario × arm × trial)`: `runs/<timestamp>/<scenario>-<arm>-<trial>.jsonl`. Event types:

```jsonc
{"event": "run_start",   "scenario": "E", "arm": "vision", "trial": 1,
                         "model": "claude-opus-4-7",
                         "kde_mcp_version": "<from /healthz>",
                         "t": "2026-..."}
{"event": "setup_done",  "fixture_pid": 12345, "window_uuid": "...",
                         "starting_geometry": [...]}
{"event": "step",        "n": 0, "t_request_sent": ..., "t_response_received": ...,
                         "usage": {"input_tokens": 1234,
                                   "cache_read_input_tokens": 0,
                                   "cache_creation_input_tokens": 0,
                                   "output_tokens": 87},
                         "tool_uses": [{"name": "left_click", "input": {...},
                                        "t_start": ..., "t_end": ...,
                                        "result_kind": "ok"}],
                         "claude_text": "I'll click the button labeled 3."}
{"event": "bridge_gap",  "n": 4, "action": "hold_key",
                         "details": {"duration_s": 2.0}}
{"event": "budget_check","cumulative_usd": 0.087, "remaining_usd": 0.413}
{"event": "verify",      "result": "pass",
                         "details": {"clicked_index": 3}}
{"event": "run_end",     "classification": "pass",
                         "total_usd": 0.142,
                         "total_wallclock_ms": 8420,
                         "totals": {"in": 9831, "out": 412, "image": 18432}}
```

`usage` mirrors Anthropic response fields directly so prompt-cache hit-rate is free.

### 4.10 Classification taxonomy

Each `(scenario × arm × trial)` ends in one of:

| Class | Definition |
|---|---|
| `pass` | `verify()` returned true. |
| `fail-tool-use-gap` | kde-mcp **exposes** the required data today; Claude didn't reach for it. Diagnosis must name the tool. |
| `fail-tool-design-gap` | kde-mcp does **not** expose the data today; a new kde-mcp tool plausibly could. |
| `fail-essential` | Required data is not accessible from any current OR hypothetical accessibility surface; pixels/OCR is the only path. |
| `timeout-budget` | Hit the $0.50 cap before `verify()` passed. Sub-classify: looping vs needed-more-iterations. **Excluded from win/loss tally** — reported separately as cost-prohibitive finding. |
| `timeout-steps` | Hit the 50-step runaway cap. Should never fire; if it does, root-cause it. |

Note: `bridge-gap` is **not** a terminal classification — it's an annotation event logged when Claude reaches for an unmapped computer-use action during a run. A run that hits a bridge-gap and still completes is classified by `verify()`; a run that fails due to inability to recover from a bridge-gap is classified `fail-essential` with `bridge-gap` annotated. The summary reports bridge-gap counts as decision-relevant context (cost of running vision-on-kde-mcp in production), not hidden as methodology noise.

Tiebreak: pass/fail axis only. Within "fail," failure-mode is reported per-trial; diagnosis names the dominant mode and notes variance.

### 4.11 Diagnosis protocol

After all runs land, draft the diagnosis in a **fresh `claude` CLI session** loaded with only the JSONL logs from `runs/<ts>/` and §4.10's taxonomy excerpt — explicitly NOT this design doc, scenario YAML, or the harness source. Every claim in the diagnosis must **quote a specific event id**, e.g., `[E-vision-trial1-step-4]`. This forces grounding and makes user review tractable: the user can spot-check the quoted events without reading the entire log corpus.

No LLM-as-judge automated classification. Self-evaluation bias on the vision arm.

### 4.12 Output: `summary.md`

```markdown
# kde-mcp adversarial probe — 2026-MM-DD

## Scope of inference
[Disclaimer from §4.1]

## Summary table
| Scenario | Arm A (kde-mcp) | Arm B (vision) | Notes |
|---|---|---|---|
| E — canvas buttons       | fail-essential (2/2)            | pass (2/2)              | AT-SPI has no name on <canvas>; vision reads digits directly. |
| Q — Qt no-accessibleName | pass (2/2 via OCR escape hatch) | pass (2/2)              | read_text recovered names; ~2 extra steps. |
| D — drag precision       | pass (2/2)                      | pass (2/2)              | Both used coords; no measurable precision gap. |
| R — chart read           | pass (2/2 via OCR)              | pass (2/2)              | OCR correct; vision marginally faster. |

## Per-scenario diagnosis
[3 paragraphs each, every claim quoting event ids]

## Bridge gaps observed
[Vision-arm computer-use actions the bridge didn't implement; how this would impact a production deployment]

## Verdict
[Counts per classification × the §2 decision rule → concrete next-step recommendation]

## Cost
| | Arm A | Arm B |
| Total spend | $X.XX | $Y.YY |
| Avg per scenario | $0.0X | $0.YY |
```

### 4.13 Operational concerns

| # | Concern | Mitigation |
|---|---|---|
| 1 | Policy-gate notification spam | Fixtures set their KWin app-class explicitly (PySide6: `app.setApplicationName("kde-mcp-probe-qt-noname")` etc.; HTML opens via `brave --class=kde-mcp-probe-canvas <url>` per Brave's window-class flag). The harness's `setup()` queries kde-mcp `window_query` after launch and prints the actual class string for first-run policy.toml curation. Then user adds the four `kde-mcp-probe-*` classes (plus `org.kde.gwenview` for **R**) to `policy.toml`'s auto-approve tier. Documented in `harness/README.md`. |
| 2 | Lease contention if parallel runs | Runs are serial. CLI refuses second run while one is in progress (advisory file lock under `runs/.lock`). |
| 3 | Leftover fixture processes between runs | `setup()` kills any fixture PID matching the fixture's class before launching; fails loudly if cleanup fails. |
| 4 | Cumulative credit overrun | Spend is tracked from `response.usage` fields (Anthropic per-token rates × current model published prices, captured in `harness/pricing.py`). A run-session starts with `BUDGET_SESSION_USD = 30.0` (override via env). Abort when consumed > `0.85 × BUDGET_SESSION_USD`. Persist running total in `runs/<ts>/spend.json`; surface in `summary.md`. |
| 5 | Reproducibility — model version | Pin `claude-opus-4-7` in `scenarios/*.yaml`; capture full model id in `run_start`. |
| 6 | Reproducibility — kde-mcp version | Capture `/healthz` response at `run_start`. **Pre-requisite small PR:** today `/healthz` returns `{ok, uptime_s, sessions}` (`src/transport/daemon.rs::healthz`). Add `version: env!("CARGO_PKG_VERSION")` and `git_sha: option_env!("KDE_MCP_GIT_SHA")` so runs can be tied to a specific binary. Lands before harness Phase 1. |
| 7 | Focus / window-position assumptions | `setup()` launches fixture, then `window_control(focus)`, then captures starting geometry. Asserts non-zero geometry before proceeding. |
| 8 | Privacy in logs | Fixtures **must not** contain real secrets. `runs/` is gitignored at repo root. |
| 9 | API key handling | `ANTHROPIC_API_KEY` from env. `harness/README.md` documents. Smoke test fails fast on missing key. |
| 10 | Polluting normal audit log | Set env `KDE_MCP_SESSION_TAG=harness` (small kde-mcp PR to record into audit). For now, harness runs are distinguishable by fixture class names. |

---

## 5. Cost projection

| Item | Cost |
|---|---|
| Phase 0 smoke | ~$0.01 |
| Study 1 (latency) | $0 — no LLM |
| Study 2 — 4 scenarios × 2 arms × 2 trials | ~$8–14 |
| Tiebreaker reserve (4 scenarios × 1 extra trial worst case) | ~$2–4 |
| **Total Study 2** | **~$10–18** |

Vision arm dominates cost (screenshots are tokenised as images at vision rate). kde-mcp arm's per-prompt tool-definition overhead (12 schemas) is largely amortised by prompt caching — the harness enables `cache_control: ephemeral` on the tools block, so steps 2+ pay cache-read rates (~10% of full input rate) for the definitions. Caching is reported in `usage.cache_read_input_tokens` and surfaced in `summary.md`.

---

## 6. Out of scope (deliberate)

- **Big A/B / statistical comparison.** Defer until probe reveals ambiguity worth resolving with bigger n.
- **Real-app scenarios** (Slack, Discord, VS Code). Reproducibility too costly; backlog spot-check.
- **LLM-as-judge for diagnosis.** Self-evaluation bias.
- **CI integration.** Research tool, not a regression gate.
- **Streaming-mode API support.** Non-streaming is symmetric and sufficient.
- **Other Claude models.** Opus 4.7 only; Sonnet/Haiku comparison is a separate study.
- **Other compositors / X11.** ADR-0002: KDE Wayland only.
- **Multi-monitor scenarios.** Single-monitor assumption.
- **Permanent in-tree harness.** Research-grade. After the probe, the spec permits archiving or moving `harness/` out of tree.

---

## 7. Future work (informed by results)

- If `fail-essential` dominates: design pixels-augmented mode (a new spec; integrates Claude vision into kde-mcp tool responses).
- If `fail-tool-design-gap` dominates: design specific journey tools (start with the most-named gap in diagnosis).
- If `fail-tool-use-gap` dominates: design a tool-description audit + tuning pass against current schemas.
- Mixed / inconclusive: publish case studies, return to this decision in 3 months with new data.

---

## 8. References

- [`docs/adr/0002-kde-only-scope.md`](../adr/0002-kde-only-scope.md) — Accepted.
- [`docs/adr/0005-accessibility-tree-first.md`](../adr/0005-accessibility-tree-first.md) — Accepted. The bet this study tests.
- [`docs/adr/0013-remove-shell-and-file-tools.md`](../adr/0013-remove-shell-and-file-tools.md) — Accepted. MCP-purpose rule informing the analyzer-out-of-tree choice.
- [`docs/specs/2026-05-13-kde-computer-use-mcp-design.md`](2026-05-13-kde-computer-use-mcp-design.md) §15 — Open questions deferred at MVP.
- [Anthropic Computer Use Tool documentation](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool).
