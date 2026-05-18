import { test } from "node:test";
import { strict as assert } from "node:assert";
import { renderReport } from "../src/report.ts";
import type { ToolStats } from "../src/aggregate.ts";

const STATS: ReadonlyMap<string, ToolStats> = new Map([
  ["observe", {
    tool: "observe",
    n: 312,
    total_p50_ms: 45,
    total_p95_ms: 98,
    total_p99_ms: 140,
    phases_p95_ms: { rmcp: 2, policy: 1, impl: 92, audit: 3 },
    dominant_phase_p95: "impl",
  }],
  ["screenshot", {
    tool: "screenshot",
    n: 87,
    total_p50_ms: 220,
    total_p95_ms: 480,
    total_p99_ms: 720,
    phases_p95_ms: { rmcp: 2, policy: 1, impl: 470, audit: 5 },
    dominant_phase_p95: "impl",
  }],
]);

test("renderReport produces a markdown table with the expected headers", () => {
  const md = renderReport(STATS, { sinceLabel: "7 days", endingLabel: "2026-05-19" });
  assert.match(md, /# kde-mcp latency profile — 7 days ending 2026-05-19/);
  assert.match(md, /\| Tool\s+\|\s+n\s+\|\s+p50 ms\s+\|\s+p95 ms\s+\|\s+p99 ms\s+\|\s+dominant phase \(p95\)\s+\|/);
});

test("renderReport includes each tool row", () => {
  const md = renderReport(STATS, { sinceLabel: "7 days", endingLabel: "2026-05-19" });
  assert.match(md, /\| observe\s+\|\s+312\s*\|\s+45\s*\|\s+98\s*\|\s+140\s*\| impl/);
  assert.match(md, /\| screenshot\s+\|\s+87\s*\|\s+220\s*\|\s+480\s*\|\s+720\s*\| impl/);
});

test("renderReport reports total N across all tools", () => {
  const md = renderReport(STATS, { sinceLabel: "7 days", endingLabel: "2026-05-19" });
  assert.match(md, /N = 399 tool calls across 2 tools/);
});
