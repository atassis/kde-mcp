import type { AuditRecord } from "./parse.ts";

export interface ToolStats {
  tool: string;
  n: number;
  total_p50_ms: number;
  total_p95_ms: number;
  total_p99_ms: number;
  // Per-phase p95 in ms, keyed by phase delta name
  phases_p95_ms: {
    rmcp: number;        // t_policy_done - t_received  (≈ deserialise + policy enter)
    policy: number;      // t_impl_start - t_policy_done
    impl: number;        // t_impl_done - t_impl_start
    audit: number;       // t_audit_written - t_impl_done
  };
  dominant_phase_p95: keyof ToolStats["phases_p95_ms"];
}

/** Nearest-rank percentile on an unsorted sample. p in [0, 100]. */
export function percentile(xs: number[], p: number): number {
  if (xs.length === 0) return 0;
  const sorted = [...xs].sort((a, b) => a - b);
  const rank = Math.max(1, Math.ceil((p / 100) * sorted.length));
  return sorted[rank - 1]!;
}

function nsToMs(ns: number): number {
  return Math.floor(ns / 1_000_000);
}

export function aggregate(records: AuditRecord[]): Map<string, ToolStats> {
  const byTool = new Map<string, AuditRecord[]>();
  for (const r of records) {
    if (!r.timings) continue;
    const bucket = byTool.get(r.tool) ?? [];
    bucket.push(r);
    byTool.set(r.tool, bucket);
  }

  const out = new Map<string, ToolStats>();
  for (const [tool, rs] of byTool) {
    const totals = rs.map((r) => r.timings!.t_audit_written_ns);
    const rmcp = rs.map((r) => r.timings!.t_policy_done_ns - r.timings!.t_received_ns);
    const policy = rs.map((r) => r.timings!.t_impl_start_ns - r.timings!.t_policy_done_ns);
    const impl = rs.map((r) => r.timings!.t_impl_done_ns - r.timings!.t_impl_start_ns);
    const audit = rs.map((r) => r.timings!.t_audit_written_ns - r.timings!.t_impl_done_ns);

    const phases_p95_ms = {
      rmcp: nsToMs(percentile(rmcp, 95)),
      policy: nsToMs(percentile(policy, 95)),
      impl: nsToMs(percentile(impl, 95)),
      audit: nsToMs(percentile(audit, 95)),
    };

    let dominant: keyof typeof phases_p95_ms = "rmcp";
    let max = -1;
    for (const k of Object.keys(phases_p95_ms) as Array<keyof typeof phases_p95_ms>) {
      if (phases_p95_ms[k] > max) {
        max = phases_p95_ms[k];
        dominant = k;
      }
    }

    out.set(tool, {
      tool,
      n: rs.length,
      total_p50_ms: nsToMs(percentile(totals, 50)),
      total_p95_ms: nsToMs(percentile(totals, 95)),
      total_p99_ms: nsToMs(percentile(totals, 99)),
      phases_p95_ms,
      dominant_phase_p95: dominant,
    });
  }
  return out;
}
