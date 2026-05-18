import { test } from "node:test";
import { strict as assert } from "node:assert";
import { aggregate, percentile } from "../src/aggregate.ts";
import type { AuditRecord } from "../src/parse.ts";

function fixture(tool: string, impl_done_ns: number): AuditRecord {
  return {
    tool,
    ts: "2026-05-19T10:00:00Z",
    timings: {
      t_received_ns: 0,
      t_policy_done_ns: 1_000_000,
      t_impl_start_ns: 1_100_000,
      t_impl_done_ns: impl_done_ns,
      t_audit_written_ns: impl_done_ns + 500_000,
    },
  };
}

test("percentile of a sorted array", () => {
  const xs = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  assert.equal(percentile(xs, 50), 5);  // nearest-rank: ceil(0.5 * 10) = 5
  assert.equal(percentile(xs, 95), 10); // ceil(0.95 * 10) = 10
  assert.equal(percentile(xs, 99), 10);
});

test("percentile handles a single value", () => {
  assert.equal(percentile([42], 95), 42);
});

test("aggregate buckets records by tool and emits p50/p95/p99", () => {
  const records = [
    fixture("observe", 10_000_000),
    fixture("observe", 20_000_000),
    fixture("observe", 30_000_000),
    fixture("observe", 100_000_000),
    fixture("screenshot", 200_000_000),
  ];
  const result = aggregate(records);
  const observe = result.get("observe")!;
  assert.equal(observe.n, 4);
  // p50 of [10, 20, 30, 100] ms (nearest-rank): ceil(0.5*4) = 2 → 20 ms
  assert.equal(observe.total_p50_ms, 20);
  // p95 (ceil(0.95*4) = 4) → 100 ms
  assert.equal(observe.total_p95_ms, 100);

  const ss = result.get("screenshot")!;
  assert.equal(ss.n, 1);
});

test("aggregate skips records without timings", () => {
  const records: AuditRecord[] = [
    fixture("observe", 10_000_000),
    { tool: "observe", ts: "2026-05-19T10:00:00Z" },
  ];
  const result = aggregate(records);
  assert.equal(result.get("observe")!.n, 1);
});
