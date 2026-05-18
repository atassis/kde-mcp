import { test } from "node:test";
import { strict as assert } from "node:assert";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { parseJsonl } from "../src/parse.ts";

const FIXTURE = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "fixtures",
  "sample.jsonl"
);

test("parseJsonl yields every line as a record", async () => {
  const records = [];
  for await (const r of parseJsonl(FIXTURE)) records.push(r);
  assert.equal(records.length, 4);
});

test("parseJsonl preserves the tool name", async () => {
  const records = [];
  for await (const r of parseJsonl(FIXTURE)) records.push(r);
  assert.equal(records[0]!.tool, "observe");
  assert.equal(records[2]!.tool, "screenshot");
});

test("parseJsonl parses timings when present", async () => {
  const records = [];
  for await (const r of parseJsonl(FIXTURE)) records.push(r);
  assert.equal(records[0]!.timings?.t_impl_done_ns, 45_000_000);
});

test("parseJsonl leaves timings undefined when absent", async () => {
  const records = [];
  for await (const r of parseJsonl(FIXTURE)) records.push(r);
  assert.equal(records[3]!.timings, undefined);
});
