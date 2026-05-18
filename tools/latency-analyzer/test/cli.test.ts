import { test } from "node:test";
import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { parseSince, parseArgs } from "../src/cli.ts";

const CLI = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "cli.ts"
);

/** Run the CLI as a subprocess. Returns stdout. */
function runCli(args: string[]): string {
  const r = spawnSync(
    process.execPath,
    ["--import", "tsx", CLI, ...args],
    { encoding: "utf8" }
  );
  if (r.status !== 0) {
    throw new Error(`cli exited ${r.status}: ${r.stderr}`);
  }
  return r.stdout;
}

/** Build a tempdir with a 3-line jsonl: one OLD, two RECENT. */
function jsonlFixtureWithCutoffData(): { path: string; cleanup: () => void } {
  const dir = mkdtempSync(path.join(os.tmpdir(), "kdemcp-cli-"));
  const file = path.join(dir, "audit.jsonl");
  const old = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString();
  const recentA = new Date(Date.now() - 10 * 60 * 1000).toISOString();
  const recentB = new Date(Date.now() - 5 * 60 * 1000).toISOString();
  const timings = {
    t_received_ns: 0,
    t_policy_done_ns: 1_000_000,
    t_impl_start_ns: 1_100_000,
    t_impl_done_ns: 50_000_000,
    t_audit_written_ns: 50_500_000,
  };
  const line = (tool: string, ts: string) =>
    JSON.stringify({ tool, ts, timings });
  writeFileSync(
    file,
    [line("ancient_tool", old), line("recent_tool", recentA), line("recent_tool", recentB), ""].join("\n")
  );
  return { path: file, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test("parseSince accepts 7d / 1h / all", () => {
  assert.equal(parseSince("7d"), 7 * 24 * 60 * 60 * 1000);
  assert.equal(parseSince("1h"), 60 * 60 * 1000);
  assert.equal(parseSince("all"), Number.POSITIVE_INFINITY);
});

test("parseSince rejects malformed input", () => {
  assert.throws(() => parseSince("seven days"));
});

test("parseArgs reads --since and --audit", () => {
  const args = parseArgs(["--since", "1h", "--audit", "/tmp/x.jsonl"]);
  assert.equal(args.sinceMs, 60 * 60 * 1000);
  assert.equal(args.auditPath, "/tmp/x.jsonl");
});

test("parseArgs defaults", () => {
  const args = parseArgs([]);
  assert.equal(args.sinceMs, Number.POSITIVE_INFINITY);
  assert.match(args.auditPath, /audit\.jsonl$/);
});

test("CLI --since all keeps every timings-bearing record", () => {
  const { path: file, cleanup } = jsonlFixtureWithCutoffData();
  try {
    const out = runCli(["--since", "all", "--audit", file]);
    // 3 records, all with timings → N = 3
    assert.match(out, /N = 3 tool calls/);
    assert.match(out, /ancient_tool/);
    assert.match(out, /recent_tool/);
  } finally {
    cleanup();
  }
});

test("CLI --since 1h filters out records older than the cutoff", () => {
  // This is the test that would have caught the timestamp/ts field-name
  // mismatch in the cross-cutting review of feat/study-1-latency.
  const { path: file, cleanup } = jsonlFixtureWithCutoffData();
  try {
    const out = runCli(["--since", "1h", "--audit", file]);
    // Only the two recent records survive → N = 2, one tool
    assert.match(out, /N = 2 tool calls/);
    assert.match(out, /recent_tool/);
    assert.doesNotMatch(out, /ancient_tool/);
  } finally {
    cleanup();
  }
});
