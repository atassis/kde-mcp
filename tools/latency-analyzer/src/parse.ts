import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";

export interface AuditTimings {
  t_received_ns: number;
  t_policy_done_ns: number;
  t_impl_start_ns: number;
  t_impl_done_ns: number;
  t_audit_written_ns: number;
}

export interface AuditRecord {
  tool: string;
  /** ISO-8601 wall-clock written by the daemon. Matches the Rust
   *  `AuditRecord.ts` field. (Originally typed as `timestamp` — a wrong
   *  field name caught by the cross-cutting review of feat/study-1-latency;
   *  the bug silently disabled the --since filter against real logs.) */
  ts: string;
  timings?: AuditTimings;
}

export async function* parseJsonl(
  path: string
): AsyncIterableIterator<AuditRecord> {
  const stream = createReadStream(path, { encoding: "utf8" });
  const rl = createInterface({ input: stream, crlfDelay: Infinity });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    yield JSON.parse(trimmed) as AuditRecord;
  }
}
