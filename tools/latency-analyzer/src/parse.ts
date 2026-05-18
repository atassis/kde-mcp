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
  timestamp: string;
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
