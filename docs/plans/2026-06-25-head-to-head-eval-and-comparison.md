# Planned: head-to-head evaluation & Linux-MCP comparison

> **Status: planned, not yet executed (2026-06-25).** This document scopes work that has not been done. Until it is, the comparisons elsewhere in this repo are design- and source-level only — see the honesty note below.

## Goal

Produce an *honest, measured* comparison of kde-mcp against the other desktop-control MCP servers in the Linux ecosystem — primarily [kwin-mcp](https://github.com/) (the closest sibling) — and against Anthropic's `computer-use`, with pros and cons stated plainly, including kde-mcp's own weaknesses.

## Honesty note (applies until this work lands)

Comparisons are reported at three levels of rigor, and each claim should be labelled with which one it is:

1. **Measured (head-to-head).** Today this covers only Anthropic's `computer-use`, via the adversarial-probe harness. No performance number is claimed for any other project.
2. **Source-read (read their code, did not run head-to-head).** kwin-mcp and others — architecture, tool surface, and mechanism only. No reliability or performance claims.
3. **Catalog (status survey).** The remaining projects — maintained / archived / Wayland-support status only. See `docs/research/prior-art.md`.

We have **not** run kwin-mcp or the other projects in a head-to-head benchmark. That is the gap this plan closes.

## Approach

1. Stand up kwin-mcp in a comparable KDE Plasma 6 / Wayland environment.
2. Run the same adversarial-probe fixtures (canvas/no-DOM buttons, no-`accessibleName` widgets, drag precision, rasterized chart) through both kde-mcp and kwin-mcp, plus the Anthropic baseline already wired up.
3. Record raw results (per-fixture pass/fail, token cost, latency) with no cherry-picking.
4. Write up a balanced pros/cons table: where kde-mcp wins by design, where it is weaker (maturity, single-user testing to date, KDE-only scope), and where the others win (breadth, maintenance, ease of contribution).

## Deliverable

An expanded `docs/comparison.md` (or a follow-up to `docs/comparison-with-anthropic-computer-use.md`) covering the full Linux landscape, with the measured/source-read/catalog labelling preserved so the reader always knows the evidence level behind each claim.
