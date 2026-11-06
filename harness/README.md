# kde-mcp adversarial-probe harness

Early scaffold for Study 2 of the kde-mcp evaluation
(`docs/specs/2026-05-19-evaluation-and-latency-design.md` §4).
Only the API capability smoke and its offline tests are implemented.
The agent loop, fixtures, policy setup, budget enforcement, scenario runner,
and run-log aggregation are not implemented yet.

## Install

    cd harness
    python -m venv .venv
    .venv/bin/pip install -e '.[dev]'

## Offline tests

    .venv/bin/python -m pytest

The live API test is skipped unless `HARNESS_LIVE_SMOKE=1` is explicitly set.
Do not enable it as part of ordinary repository verification.

## Explicit live capability smoke

This makes a **paid Anthropic API request** and requires an API key in
`ANTHROPIC_API_KEY` with access to the configured model and computer-use beta.
No desktop capture or input is performed by this smoke.

    .venv/bin/harness-smoke

Or, without the console entry point:

    .venv/bin/python -m harness.smoke

The smoke requests a screenshot action and checks whether the API response
contains a `tool_use` block. It does not execute the requested action or send
a tool result back, so it does not verify a complete computer-use round trip.
Model access and beta availability depend on the API account; failure should
be resolved before building or running the remaining evaluation harness.

## Planned evaluation

The design calls for four fixtures, two tool arms (kde-mcp and the Anthropic
computer-use tool), repeated trials, budget limits, and per-run JSONL logs.
See the evaluation spec for the intended scenarios and classification rules;
these are requirements, not currently available commands. `runs/` is ignored
so future screenshots, audit excerpts, and run logs are not published by default.
