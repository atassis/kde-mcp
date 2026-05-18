import path from "node:path";
import { homedir } from "node:os";
import { parseJsonl, type AuditRecord } from "./parse.ts";
import { aggregate } from "./aggregate.ts";
import { renderReport } from "./report.ts";

export interface Args {
  sinceMs: number;
  auditPath: string;
}

const DEFAULT_AUDIT_PATH = path.join(
  homedir(),
  ".local",
  "state",
  "kde-mcp",
  "audit.jsonl"
);

const SINCE_RE = /^(\d+)([hd])$/;

export function parseSince(s: string): number {
  if (s === "all") return Number.POSITIVE_INFINITY;
  const m = SINCE_RE.exec(s);
  if (!m) throw new Error(`unrecognised --since value: ${s} (use 7d / 1h / all)`);
  const n = Number(m[1]);
  return m[2] === "d" ? n * 24 * 60 * 60 * 1000 : n * 60 * 60 * 1000;
}

export function parseArgs(argv: string[]): Args {
  let sinceMs = Number.POSITIVE_INFINITY;
  let auditPath = DEFAULT_AUDIT_PATH;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--since" && i + 1 < argv.length) {
      sinceMs = parseSince(argv[++i]!);
    } else if (argv[i] === "--audit" && i + 1 < argv.length) {
      auditPath = argv[++i]!;
    }
  }
  return { sinceMs, auditPath };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const cutoff = Date.now() - (args.sinceMs === Number.POSITIVE_INFINITY ? Number.POSITIVE_INFINITY : args.sinceMs);

  const records: AuditRecord[] = [];
  for await (const r of parseJsonl(args.auditPath)) {
    const t = Date.parse(r.timestamp);
    if (!Number.isFinite(t) || t >= cutoff) records.push(r);
  }

  const stats = aggregate(records);
  const sinceLabel =
    args.sinceMs === Number.POSITIVE_INFINITY ? "all time" : describeMs(args.sinceMs);
  const today = new Date().toISOString().slice(0, 10);
  process.stdout.write(renderReport(stats, { sinceLabel, endingLabel: today }));
}

function describeMs(ms: number): string {
  const days = Math.round(ms / (24 * 60 * 60 * 1000));
  if (days >= 1) return `${days} day${days === 1 ? "" : "s"}`;
  const hours = Math.round(ms / (60 * 60 * 1000));
  return `${hours} hour${hours === 1 ? "" : "s"}`;
}

// Only run main() when invoked directly, not when imported by tests
const invokedDirect = import.meta.url === `file://${process.argv[1]}`;
if (invokedDirect) {
  main().catch((err) => {
    process.stderr.write(`error: ${err.message}\n`);
    process.exit(1);
  });
}
