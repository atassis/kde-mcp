import { test } from "node:test";
import { strict as assert } from "node:assert";
import { parseSince, parseArgs } from "../src/cli.ts";

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
