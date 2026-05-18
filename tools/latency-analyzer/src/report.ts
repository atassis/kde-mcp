import type { ToolStats } from "./aggregate.ts";

export interface RenderOptions {
  sinceLabel: string;   // e.g., "7 days"
  endingLabel: string;  // ISO-ish date, e.g., "2026-05-19"
}

export function renderReport(
  stats: ReadonlyMap<string, ToolStats>,
  opts: RenderOptions
): string {
  const totalN = [...stats.values()].reduce((acc, s) => acc + s.n, 0);
  const rows = [...stats.values()]
    .sort((a, b) => b.total_p95_ms - a.total_p95_ms)
    .map(
      (s) =>
        `| ${s.tool.padEnd(14)}| ${String(s.n).padStart(4)}| ${String(s.total_p50_ms).padStart(6)}| ${String(s.total_p95_ms).padStart(6)}| ${String(s.total_p99_ms).padStart(6)}| ${s.dominant_phase_p95}`
    )
    .join("\n");

  const phaseRows = [...stats.values()]
    .map((s) => {
      const p = s.phases_p95_ms;
      return `| ${s.tool.padEnd(14)}| ${String(p.rmcp).padStart(4)}| ${String(p.policy).padStart(6)}| ${String(p.impl).padStart(4)}| ${String(p.audit).padStart(5)}`;
    })
    .join("\n");

  return `# kde-mcp latency profile — ${opts.sinceLabel} ending ${opts.endingLabel}

N = ${totalN} tool calls across ${stats.size} tools.

| Tool          |    n | p50 ms | p95 ms | p99 ms | dominant phase (p95)
|---------------|-----:|-------:|-------:|-------:|---------------------
${rows}

## Phase-level p95 (ms)

| Tool          | rmcp | policy | impl | audit
|---------------|-----:|-------:|-----:|-----:
${phaseRows}
`;
}
