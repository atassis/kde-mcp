# Study 1 — Latency Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-phase latency instrumentation to kde-mcp's audit log and a TypeScript analyzer that emits a markdown report from `audit.jsonl`.

**Architecture:** Two pieces along the producer/consumer line. Producer is in-tree Rust changes to `src/audit.rs`, `src/server.rs::dispatch_audited`, and `src/transport/daemon.rs::healthz`. Consumer is a new TypeScript package at `tools/latency-analyzer/` that reads the JSONL, computes percentiles, and emits markdown. The two share no code — only the JSONL contract.

**Tech Stack:** Rust (existing crate, edition 2021, MSRV 1.94, tokio, serde, axum). TypeScript with Node 22+ and the built-in test runner (`node --test`). No frameworks.

**Spec reference:** [`docs/specs/2026-05-19-evaluation-and-latency-design.md`](../specs/2026-05-19-evaluation-and-latency-design.md) §3 and §4.13 row 6.

---

## File Structure

**Created:**
- `tools/latency-analyzer/package.json`
- `tools/latency-analyzer/tsconfig.json`
- `tools/latency-analyzer/.gitignore`
- `tools/latency-analyzer/README.md`
- `tools/latency-analyzer/src/parse.ts` — streaming JSONL parser
- `tools/latency-analyzer/src/aggregate.ts` — percentile math
- `tools/latency-analyzer/src/report.ts` — markdown emitter
- `tools/latency-analyzer/src/cli.ts` — argv entrypoint
- `tools/latency-analyzer/test/parse.test.ts`
- `tools/latency-analyzer/test/aggregate.test.ts`
- `tools/latency-analyzer/test/report.test.ts`
- `tools/latency-analyzer/test/cli.test.ts`
- `tools/latency-analyzer/test/fixtures/sample.jsonl`
- `build.rs` (repo root) — emits `KDE_MCP_GIT_SHA` env var at compile time

**Modified:**
- `src/audit.rs` — add optional `AuditTimings` struct to `AuditRecord`
- `src/server.rs` — capture five timestamps in `dispatch_audited`, store on the audit record
- `src/transport/daemon.rs` — `healthz()` returns `version` + `git_sha`
- `tests/integration/daemon_http.rs` — assert healthz fields
- `tests/integration.rs` — wire new test modules if needed (only if a new test file is added; otherwise leave alone)
- `Cargo.toml` — add `build = "build.rs"` to `[package]` (only if not already present)

**Out of scope of this plan** (intentionally deferred to Plan 2):
- Anything under `harness/`
- `KDE_MCP_SESSION_TAG` env-var support (spec §4.13 row 10)
- `policy.toml` auto-approve curation (a runbook step, not implementation)

---

## Task 1: Add `version` and `git_sha` to `/healthz`

This is a prerequisite for Plan 2's reproducibility row, but useful on its own (`curl /healthz` will tell you which binary is running).

**Files:**
- Create: `build.rs` (repo root)
- Modify: `Cargo.toml` — add `build = "build.rs"` to `[package]` if missing
- Modify: `src/transport/daemon.rs:248-254` (the `healthz` handler)
- Modify: `tests/integration/daemon_http.rs` — extend the existing `healthz_reports_ok_without_auth` test

- [ ] **Step 1: Read the existing healthz test to understand the call pattern**

Run: `grep -n "healthz_reports_ok" tests/integration/daemon_http.rs`
Expected: one match locating the existing test that asserts `ok: true` etc. Read it.

- [ ] **Step 2: Extend the existing test to assert version + git_sha**

Modify `tests/integration/daemon_http.rs` — find the `healthz_reports_ok_without_auth` test. After the existing assertions on `ok` / `uptime_s` / `sessions`, append:

```rust
    let version = body.get("version").and_then(|v| v.as_str());
    assert!(version.is_some(), "healthz must report version");
    assert!(
        version.unwrap().chars().any(|c| c.is_ascii_digit()),
        "healthz version should contain at least one digit"
    );
    let git_sha = body.get("git_sha").and_then(|v| v.as_str());
    assert!(git_sha.is_some(), "healthz must report git_sha");
    // git_sha is either a hex sha (likely short) or the literal "unknown"
    // when build was not run inside a git tree
    let s = git_sha.unwrap();
    assert!(
        s == "unknown" || s.chars().all(|c| c.is_ascii_hexdigit()),
        "git_sha should be hex or 'unknown', got: {s}"
    );
```

- [ ] **Step 3: Run the test — expect failure (fields don't exist yet)**

Run: `cargo test --test integration daemon_http::healthz_reports_ok_without_auth -- --nocapture`
Expected: FAIL on the new `version.is_some()` assertion.

- [ ] **Step 4: Create `build.rs` at the repo root**

```rust
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=KDE_MCP_GIT_SHA={sha}");
}
```

- [ ] **Step 5: Ensure Cargo.toml declares the build script**

Run: `grep -n '^build' Cargo.toml || echo MISSING`

If `MISSING`, add `build = "build.rs"` to `[package]` in `Cargo.toml` (right after `name`/`version`).

- [ ] **Step 6: Update the `healthz` handler**

Modify `src/transport/daemon.rs:248-254` — replace the body of `healthz`:

```rust
async fn healthz(State(state): State<HealthState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "uptime_s": state.started_at.elapsed().as_secs(),
        "sessions": state.sessions.load(Ordering::Relaxed),
        "version": env!("CARGO_PKG_VERSION"),
        "git_sha": env!("KDE_MCP_GIT_SHA"),
    }))
}
```

Note: `env!` (not `option_env!`) because `build.rs` always sets the var.

- [ ] **Step 7: Run the test — expect pass**

Run: `cargo test --test integration daemon_http::healthz_reports_ok_without_auth -- --nocapture`
Expected: PASS.

- [ ] **Step 8: Run the full test suite + clippy + fmt**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean fmt, no clippy warnings, all tests pass.

- [ ] **Step 9: Commit**

```bash
git add build.rs Cargo.toml src/transport/daemon.rs tests/integration/daemon_http.rs
git commit -m "feat(daemon): /healthz reports version and git_sha

Reproducibility prerequisite for upcoming evaluation harness (spec
docs/specs/2026-05-19-evaluation-and-latency-design.md §4.13 row 6):
runs need to be tied to the exact binary that produced them. version
comes from CARGO_PKG_VERSION; git_sha is set by build.rs from
'git rev-parse --short=12 HEAD' and falls back to 'unknown' outside
a git tree."
```

---

## Task 2: Inspect existing audit schema

Before modifying `AuditRecord`, read it. The memory note says timing fields may already partially exist; we need to know what we're adding vs touching.

**Files:**
- Read: `src/audit.rs`

- [ ] **Step 1: Read the audit module top-to-bottom**

Run: `wc -l src/audit.rs` and then read the whole file.
Expected: the file is small (< 500 lines). Capture what you find — specifically:
- The `AuditRecord` struct definition.
- Whether any timing/duration field is already present.
- How records are serialized (serde derives? manual?).
- Where `AuditRecord::new` or equivalent is constructed.

- [ ] **Step 2: Record the findings in a scratch note inside this plan**

This isn't a code change. Open this plan file, find the placeholder below, and replace `<<AUDIT-NOTES>>` with three bullet points capturing:
1. Existing timing fields, if any.
2. The current `AuditRecord` struct shape.
3. Where new fields would slot in without breaking serde.

- **Existing timing fields:** one — `duration_ms: Option<u64>` (line 17) with `#[serde(skip_serializing_if = "Option::is_none")]`. Coarse wall-clock total only; no phase breakdown. Task 3's `AuditTimings` is additive and doesn't conflict — `duration_ms` and `timings` can coexist (the analyzer will prefer `timings.t_audit_written_ns` when present, fall back to `duration_ms` otherwise; no Rust-side rename needed). No `Deserialize` derive on `AuditRecord` today (only `#[derive(Debug, Serialize)]` at line 9) and no `Default` impl — Task 3 must add both (`Deserialize` for the round-trip test; `Default` for the `default_for_test()` helper, OR hand-roll `default_for_test()` without the `..Default::default()` shorthand). All current Option fields use `skip_serializing_if = "Option::is_none"`, so adding `timings: Option<AuditTimings>` with the same attribute preserves wire compatibility for old readers and old records.
- **Current `AuditRecord` shape (verbatim field list):** `ts: String`, `mode: &'static str`, `tool: String`, `args: Value`, `ok: bool`, `duration_ms: Option<u64>`, `error: Option<Value>`, `vision_fallback: Option<bool>`, `session_id: Option<String>`. Constructed inline at call sites (no `AuditRecord::new` constructor) — confirmed by `impl AuditLog::write_event` at line 67 and the test fixtures at lines 175-185 and 211-222, both of which list every field explicitly. The `&'static str` on `mode` is the only non-owned field and would block a naive `#[derive(Default)]` — Task 3 should hand-write `default_for_test()` rather than relying on `Default`.
- **Where new fields slot in without breaking serde:** append `timings: Option<AuditTimings>` as the last field of `AuditRecord` (after `session_id`, line 25) with `#[serde(default, skip_serializing_if = "Option::is_none")]`. Define `AuditTimings` immediately above `AuditRecord` (after the `use` block, before line 9). Every existing call-site that constructs `AuditRecord` by listing all fields (lines 67-80 in `write_event`, lines 175-185 and 211-222 in tests) must add `timings: None` — three sites total inside `src/audit.rs`; `dispatch_audited` in `src/server.rs` is the fourth (handled by Task 4). `serde(default)` on the new field ensures old JSONL records on disk still deserialize once `Deserialize` is added.

(If the placeholder is already filled by a prior pass, leave it.)

- [ ] **Step 3: No commit — this is exploration**

---

## Task 3: Extend `AuditRecord` with optional `AuditTimings`

Add a single optional sub-struct that carries five timestamps. Optional so old records remain valid; new records will populate it; the analyzer will skip records without it.

**Files:**
- Modify: `src/audit.rs`

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block at the bottom of `src/audit.rs`:

```rust
    #[test]
    fn audit_record_round_trips_with_timings() {
        let timings = AuditTimings {
            t_received_ns: 1_000_000,
            t_policy_done_ns: 1_500_000,
            t_impl_start_ns: 1_600_000,
            t_impl_done_ns: 50_000_000,
            t_audit_written_ns: 50_500_000,
        };
        let record = AuditRecord {
            // ... use whatever fields the existing AuditRecord requires,
            //     mirroring an existing test in this module if one exists
            timings: Some(timings),
            ..AuditRecord::default_for_test()
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: AuditRecord = serde_json::from_str(&json).unwrap();
        let t = back.timings.expect("timings present after round-trip");
        assert_eq!(t.t_received_ns, 1_000_000);
        assert_eq!(t.t_audit_written_ns, 50_500_000);
        assert!(t.t_impl_done_ns > t.t_impl_start_ns);
    }

    #[test]
    fn audit_record_round_trips_without_timings() {
        let record = AuditRecord::default_for_test();
        let json = serde_json::to_string(&record).unwrap();
        let back: AuditRecord = serde_json::from_str(&json).unwrap();
        assert!(back.timings.is_none(), "timings should remain None when omitted");
    }
```

(`default_for_test()` is a `#[cfg(test)] impl` helper — add one in step 3 if it doesn't already exist, matching the existing `AuditRecord` fields.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib audit::tests::audit_record_round_trips`
Expected: FAIL — `AuditTimings` and/or `timings` field don't exist.

- [ ] **Step 3: Add `AuditTimings` struct, `timings` field, and update derives**

Per the Task 2 audit, `AuditRecord` derives only `#[derive(Debug, Serialize)]` today, and the `mode` field is `&'static str` (which means a naive `Default` would set `mode = ""`, violating the live/dry invariant). The plan is therefore:

(a) Add `Deserialize` to the derive macro on `AuditRecord` — the round-trip tests need it.

(b) Do NOT add `Default` to `AuditRecord`. Hand-write `default_for_test()` listing every field explicitly.

(c) Add the new `AuditTimings` struct above `AuditRecord`.

(d) Add the `timings` field as the last field on `AuditRecord`.

(e) Update the three existing exhaustive `AuditRecord { ... }` constructions inside `src/audit.rs` to include `timings: None` (these were enumerated by Task 2: `AuditLog::write_event` ~line 67-80, plus two test fixtures around lines 175-185 and 211-222 — verify the current line numbers when editing).

In `src/audit.rs`, add near the existing `AuditRecord` (above it, after the `use` block):

```rust
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AuditTimings {
    pub t_received_ns: u64,
    pub t_policy_done_ns: u64,
    pub t_impl_start_ns: u64,
    pub t_impl_done_ns: u64,
    pub t_audit_written_ns: u64,
}
```

Change `#[derive(Debug, Serialize)]` on `AuditRecord` to:

```rust
#[derive(Debug, Serialize, Deserialize)]
```

Add the field at the END of the struct (after `session_id`):

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<AuditTimings>,
```

Update every exhaustive constructor of `AuditRecord` in `src/audit.rs` to include `timings: None`. Confirm the exact set with `grep -n "AuditRecord {" src/audit.rs` before editing.

Add the test-only helper at the bottom of the module:

```rust
#[cfg(test)]
impl AuditRecord {
    pub(crate) fn default_for_test() -> Self {
        Self {
            ts: "1970-01-01T00:00:00Z".into(),
            mode: "live",
            tool: "test_tool".into(),
            args: serde_json::Value::Null,
            ok: true,
            duration_ms: None,
            error: None,
            vision_fallback: None,
            session_id: None,
            timings: None,
        }
    }
}
```

(If any existing test fixture in the module already constructs a "default" record, mirror its values; the helper above mirrors `write_event`'s defaults, which is the canonical baseline.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib audit::tests::audit_record_round_trips`
Expected: PASS (both new tests).

- [ ] **Step 5: Run full suite + clippy**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/audit.rs
git commit -m "feat(audit): optional AuditTimings on AuditRecord

Five nanosecond timestamps wrapping the request lifecycle:
t_received, t_policy_done, t_impl_start, t_impl_done,
t_audit_written. Optional + skip_serializing_if(none) for
backwards compatibility with existing audit.jsonl readers.
Per spec docs/specs/2026-05-19-evaluation-and-latency-design.md §3.1."
```

---

## Task 4: Capture timestamps in `dispatch_audited`

The producer side. Walk `src/server.rs::dispatch_audited` (or wherever the per-tool dispatch lives), capture `Instant::now()` at each phase boundary, and pass the timings into the audit write.

**Files:**
- Modify: `src/server.rs`

- [ ] **Step 1: Inspect the existing dispatcher**

Run: `grep -n "fn dispatch_audited\|fn dispatch_with_audit\|audit\.append\|AuditRecord" src/server.rs`
Expected: one or two locations where audit records are constructed and emitted. Read those locations + the surrounding function signature.

- [ ] **Step 2: Write a failing integration test**

Create or extend a test under `tests/integration/` that invokes a single MCP tool (the cheapest — `tools/list` or `tools/call` of `observe` in dry-mode) and asserts that the resulting audit-log line has `timings.t_audit_written_ns > timings.t_received_ns` and all five fields present.

If no existing integration scaffold tails the audit log, write a thin helper that reads the JSONL file before and after the call.

```rust
// tests/integration/audit_timings.rs (new file)
// Driven by existing daemon_http test patterns; mirror lease.rs or
// daemon_http.rs for the bootstrap, then:

#[test]
fn audit_record_carries_five_timings_after_a_tool_call() {
    // 1. Start daemon with a temp audit-log path (mirroring existing tests)
    // 2. POST a tools/list call
    // 3. Read the last line of the audit log
    // 4. Deserialize into AuditRecord; assert timings.is_some() and ordering

    // <fill the bootstrap from the existing daemon_http test>
    let record = read_last_audit_record(&audit_path);
    let t = record.timings.expect("timings present");
    assert!(t.t_policy_done_ns >= t.t_received_ns);
    assert!(t.t_impl_start_ns >= t.t_policy_done_ns);
    assert!(t.t_impl_done_ns >= t.t_impl_start_ns);
    assert!(t.t_audit_written_ns >= t.t_impl_done_ns);
}
```

If `tests/integration.rs` requires module registration, add `mod audit_timings;` there.

- [ ] **Step 3: Run the test — expect failure**

Run: `cargo test --test integration audit_timings`
Expected: FAIL — `timings` is `None`.

- [ ] **Step 4: Wire timestamp capture in `dispatch_audited`**

In `src/server.rs`, find the function that constructs `AuditRecord` for each tool call. Wrap the four phase boundaries with `Instant::now()` captures. The fifth (`t_audit_written_ns`) is captured *after* the audit append returns. Convert each `Instant` to ns relative to the start of the request (i.e., `t_received_ns = 0`, and the rest are deltas from that). Example skeleton (adapt to the real function shape):

```rust
let request_start = std::time::Instant::now();
let t_received_ns = 0u64;

// ... existing policy gate ...
let t_policy_done_ns = request_start.elapsed().as_nanos() as u64;

let t_impl_start_ns = request_start.elapsed().as_nanos() as u64;
let tool_result = fut.await; // existing tool body
let t_impl_done_ns = request_start.elapsed().as_nanos() as u64;

// ... existing audit-record construction; populate the timings field
let timings = crate::audit::AuditTimings {
    t_received_ns,
    t_policy_done_ns,
    t_impl_start_ns,
    t_impl_done_ns,
    t_audit_written_ns: 0, // filled below
};
let mut record = AuditRecord { /* existing fields */, timings: Some(timings) };

// Append, then patch t_audit_written_ns. Since append is the bottleneck we
// want to measure, do it in two stages: append a placeholder, then update.
// SIMPLER: capture the time and write the record once, after the write
// returns. Per spec §3.1 the field is t_audit_written_ns meaning "after
// the audit record flushes" — capture *just before* flush is acceptable
// and avoids the double-write.
record.timings.as_mut().unwrap().t_audit_written_ns =
    request_start.elapsed().as_nanos() as u64;
audit.append(&record).await?;
```

(The `t_audit_written_ns` field semantics: capture immediately before `audit.append` is called. The spec says "after the audit record flushes," but capturing that exactly requires a second write or an in-memory rewrite — practical compromise is "just before flush." Document the decision in a one-line code comment.)

- [ ] **Step 5: Add a `// ` code comment explaining the t_audit_written_ns timing choice**

Single line near the capture:

```rust
// Captured just before audit.append; spec §3.1 says "after flush" but the
// flush itself is the measured operation, so exact-after would require a
// double-write. The delta within a single record is < 1 µs typically.
```

- [ ] **Step 6: Run the test — expect pass**

Run: `cargo test --test integration audit_timings`
Expected: PASS.

- [ ] **Step 7: Run full suite + clippy + fmt**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/server.rs tests/integration/audit_timings.rs tests/integration.rs
git commit -m "feat(server): capture phase timings on every audit record

Five-timestamp budget per tool call (t_received_ns through
t_audit_written_ns, all relative to request start). Feeds the
TypeScript latency analyzer landing in the next commit. Per spec
docs/specs/2026-05-19-evaluation-and-latency-design.md §3.1."
```

---

## Task 5: Scaffold the TypeScript analyzer package

**Files:**
- Create: `tools/latency-analyzer/package.json`
- Create: `tools/latency-analyzer/tsconfig.json`
- Create: `tools/latency-analyzer/.gitignore`
- Create: `tools/latency-analyzer/README.md`

- [ ] **Step 1: Create `tools/latency-analyzer/package.json`**

```json
{
  "name": "kde-mcp-latency-analyzer",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "engines": { "node": ">=22" },
  "scripts": {
    "build": "tsc",
    "test": "node --test --import tsx test/*.test.ts",
    "report": "node --import tsx src/cli.ts"
  },
  "devDependencies": {
    "tsx": "^4.19.0",
    "typescript": "^5.6.0",
    "@types/node": "^22.0.0"
  }
}
```

- [ ] **Step 2: Create `tools/latency-analyzer/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "noUncheckedIndexedAccess": true,
    "outDir": "dist"
  },
  "include": ["src/**/*.ts", "test/**/*.ts"]
}
```

(Note: `rootDir` is intentionally omitted. Setting `rootDir: "src"` would conflict with the `test/**/*.ts` include and break `npm run build` — tsc would error that test files live outside rootDir. Let tsc infer the common root from `include`.)

- [ ] **Step 3: Create `tools/latency-analyzer/.gitignore`**

```
node_modules/
dist/
coverage/
*.log
*.tsbuildinfo
```

- [ ] **Step 4: Create `tools/latency-analyzer/README.md`**

```markdown
# kde-mcp latency analyzer

Reads `~/.local/state/kde-mcp/audit.jsonl` and emits a markdown report
with p50/p95/p99 per tool, broken into the five phases captured by
the daemon (see `src/audit.rs::AuditTimings`).

## Install

    cd tools/latency-analyzer
    npm install

## Usage

    npm run report -- --since 7d > docs/latency-profile.md

Flags:
- `--since DURATION` — only consider records younger than DURATION
  (`1h`, `7d`, `30d`). Default: `all`.
- `--audit PATH` — override the default audit-log path.

## Tests

    npm test
```

- [ ] **Step 5: Install + verify tsc passes on empty src**

Run:
```bash
cd tools/latency-analyzer
npm install
mkdir -p src test
echo 'export {};' > src/parse.ts
npx tsc --noEmit
```
Expected: install succeeds, `tsc --noEmit` reports no errors.

Clean up the stub: `rm src/parse.ts` (it'll be re-created in Task 6).

- [ ] **Step 6: Commit**

```bash
git add tools/latency-analyzer/package.json tools/latency-analyzer/tsconfig.json tools/latency-analyzer/.gitignore tools/latency-analyzer/README.md tools/latency-analyzer/package-lock.json
git commit -m "feat(latency-analyzer): scaffold TypeScript package

Reads kde-mcp's audit.jsonl, emits a markdown latency profile.
Plain Node 22+ with the built-in test runner; tsx for ESM
TypeScript without a compile step. Per spec
docs/specs/2026-05-19-evaluation-and-latency-design.md §3."
```

---

## Task 6: Implement the JSONL parser

**Files:**
- Create: `tools/latency-analyzer/src/parse.ts`
- Create: `tools/latency-analyzer/test/fixtures/sample.jsonl`
- Create: `tools/latency-analyzer/test/parse.test.ts`

- [ ] **Step 1: Create the fixture**

`tools/latency-analyzer/test/fixtures/sample.jsonl`:

```
{"tool":"observe","timestamp":"2026-05-19T10:00:00Z","timings":{"t_received_ns":0,"t_policy_done_ns":2000000,"t_impl_start_ns":2100000,"t_impl_done_ns":45000000,"t_audit_written_ns":45500000}}
{"tool":"observe","timestamp":"2026-05-19T10:00:05Z","timings":{"t_received_ns":0,"t_policy_done_ns":1500000,"t_impl_start_ns":1600000,"t_impl_done_ns":98000000,"t_audit_written_ns":98300000}}
{"tool":"screenshot","timestamp":"2026-05-19T10:00:10Z","timings":{"t_received_ns":0,"t_policy_done_ns":3000000,"t_impl_start_ns":3100000,"t_impl_done_ns":220000000,"t_audit_written_ns":220400000}}
{"tool":"window_query","timestamp":"2026-05-19T10:00:11Z"}
```

The fourth line has no `timings` — must be parsed and yielded but with `timings: undefined`.

- [ ] **Step 2: Write the failing test**

`tools/latency-analyzer/test/parse.test.ts`:

```typescript
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
```

- [ ] **Step 3: Run the test — expect failure**

Run: `cd tools/latency-analyzer && npm test`
Expected: FAIL — `parseJsonl` not defined.

- [ ] **Step 4: Implement `src/parse.ts`**

```typescript
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
```

- [ ] **Step 5: Run the test — expect pass**

Run: `cd tools/latency-analyzer && npm test`
Expected: 4 tests passing.

- [ ] **Step 6: Commit**

```bash
git add tools/latency-analyzer/src/parse.ts tools/latency-analyzer/test/parse.test.ts tools/latency-analyzer/test/fixtures/sample.jsonl
git commit -m "feat(latency-analyzer): streaming JSONL parser

Async iterator over audit.jsonl. Tolerates lines with no timings
field (older records, or tools that haven't been re-instrumented
yet). Type-safe via the AuditTimings interface mirroring the
Rust struct."
```

---

## Task 7: Implement the percentile aggregator

**Files:**
- Create: `tools/latency-analyzer/src/aggregate.ts`
- Create: `tools/latency-analyzer/test/aggregate.test.ts`

- [ ] **Step 1: Write the failing test**

`tools/latency-analyzer/test/aggregate.test.ts`:

```typescript
import { test } from "node:test";
import { strict as assert } from "node:assert";
import { aggregate, percentile } from "../src/aggregate.ts";
import type { AuditRecord } from "../src/parse.ts";

function fixture(tool: string, impl_done_ns: number): AuditRecord {
  return {
    tool,
    timestamp: "2026-05-19T10:00:00Z",
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
    { tool: "observe", timestamp: "2026-05-19T10:00:00Z" },
  ];
  const result = aggregate(records);
  assert.equal(result.get("observe")!.n, 1);
});
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd tools/latency-analyzer && npm test`
Expected: FAIL — `aggregate`/`percentile` not defined.

- [ ] **Step 3: Implement `src/aggregate.ts`**

```typescript
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
  return Math.round(ns / 1_000_000);
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
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd tools/latency-analyzer && npm test`
Expected: all aggregate tests passing.

- [ ] **Step 5: Commit**

```bash
git add tools/latency-analyzer/src/aggregate.ts tools/latency-analyzer/test/aggregate.test.ts
git commit -m "feat(latency-analyzer): percentile aggregator per tool

Nearest-rank percentiles (no interpolation; matches what Prometheus/
node-exporter call 'p95'). Per-tool buckets; per-phase p95 sub-table;
'dominant phase' tag for the largest of the four phase deltas."
```

---

## Task 8: Implement the markdown reporter

**Files:**
- Create: `tools/latency-analyzer/src/report.ts`
- Create: `tools/latency-analyzer/test/report.test.ts`

- [ ] **Step 1: Write the failing test**

`tools/latency-analyzer/test/report.test.ts`:

```typescript
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
  assert.match(md, /\| Tool\s+\| n\s+\| p50 ms\s+\| p95 ms\s+\| p99 ms\s+\| dominant phase \(p95\)\s+\|/);
});

test("renderReport includes each tool row", () => {
  const md = renderReport(STATS, { sinceLabel: "7 days", endingLabel: "2026-05-19" });
  assert.match(md, /\| observe\s+\| 312\s+\| 45\s+\| 98\s+\| 140\s+\| impl/);
  assert.match(md, /\| screenshot\s+\| 87\s+\| 220\s+\| 480\s+\| 720\s+\| impl/);
});

test("renderReport reports total N across all tools", () => {
  const md = renderReport(STATS, { sinceLabel: "7 days", endingLabel: "2026-05-19" });
  assert.match(md, /N = 399 tool calls across 2 tools/);
});
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd tools/latency-analyzer && npm test`
Expected: FAIL — `renderReport` not defined.

- [ ] **Step 3: Implement `src/report.ts`**

```typescript
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
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd tools/latency-analyzer && npm test`
Expected: all report tests passing.

- [ ] **Step 5: Commit**

```bash
git add tools/latency-analyzer/src/report.ts tools/latency-analyzer/test/report.test.ts
git commit -m "feat(latency-analyzer): markdown report emitter

Pads columns for readability; sorts tools by p95 descending. Adds a
phase-level p95 breakdown table so the 'dominant phase' column can be
verified at a glance."
```

---

## Task 9: Implement the CLI entry point

**Files:**
- Create: `tools/latency-analyzer/src/cli.ts`
- Create: `tools/latency-analyzer/test/cli.test.ts`

- [ ] **Step 1: Write the failing test**

`tools/latency-analyzer/test/cli.test.ts`:

```typescript
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
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd tools/latency-analyzer && npm test`
Expected: FAIL — `parseSince`/`parseArgs` not defined.

- [ ] **Step 3: Implement `src/cli.ts`**

```typescript
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
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd tools/latency-analyzer && npm test`
Expected: all tests passing.

- [ ] **Step 5: Smoke-test the CLI against the fixture**

Run:
```bash
cd tools/latency-analyzer
node --import tsx src/cli.ts --since all --audit test/fixtures/sample.jsonl > /tmp/report.md
cat /tmp/report.md
```
Expected: a populated markdown report on stdout; observe and screenshot rows present; window_query is skipped (no timings).

- [ ] **Step 6: Commit**

```bash
git add tools/latency-analyzer/src/cli.ts tools/latency-analyzer/test/cli.test.ts
git commit -m "feat(latency-analyzer): CLI entry point

--since (7d / 1h / all) and --audit (override path). Streams via the
async parser; aggregates; writes markdown to stdout. main() guarded
by invokedDirect so the module stays import-safe under the test
runner."
```

---

## Task 10: First real run against the actual audit log

This is the end-to-end validation that the producer and consumer agree on the JSONL shape.

- [ ] **Step 1: Rebuild kde-mcp and ensure the daemon is running**

Run:
```bash
cargo build --release
~/.local/bin/kde-mcp --help  # confirms the new binary is installed at the standard path
systemctl --user is-active kde-mcp-http.service
```

If the daemon is using the old binary, restart it: `systemctl --user restart kde-mcp-http.service` (this depends on whether the binary was reinstalled via `packaging/install.sh`; if not, the next call to `/mcp` may still be served by the old binary — restart explicitly).

- [ ] **Step 2: Trigger a few tool calls to populate audit.jsonl with timings**

Run:
```bash
curl -sS -X POST http://127.0.0.1:7575/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"latency-smoke","version":"0"}}}'
```

Followed by a `tools/list` and a `tools/call` to `window_query` (cheapest non-trivial tool).

- [ ] **Step 3: Run the analyzer**

Run:
```bash
cd tools/latency-analyzer
npm run report -- --since 1h
```
Expected: markdown output on stdout. At least one tool row with non-zero p50. Skim the dominant phase column for surprises.

- [ ] **Step 4: Save the first profile**

```bash
cd tools/latency-analyzer
npm run report -- --since 7d > ../../docs/latency-profile.md
```

- [ ] **Step 5: Commit**

```bash
git add docs/latency-profile.md
git commit -m "docs: first kde-mcp latency profile

Snapshot from <today's date> against the daemon at HEAD. Re-generate
via 'cd tools/latency-analyzer && npm run report -- --since 7d' after
any change to src/server.rs, src/tools/*, or src/audit.rs (per spec
docs/specs/2026-05-19-evaluation-and-latency-design.md §3.4)."
```

---

## Self-review checklist

Run through this once after writing the plan:

- [ ] **Spec coverage:** §3.1 producer → Task 3+4. §3.1 consumer → Task 5–9. §3.2 usage → Task 5 (README) + Task 10 (real invocation). §3.3 output format → Task 8. §3.4 operational → Task 10 step 4 + README. §4.13 row 6 healthz version → Task 1. **All spec §3 sections covered.**
- [ ] **No placeholders:** every step has either runnable code, an exact command, or an inspection task whose output feeds the next step. The one explicit `<<AUDIT-NOTES>>` placeholder in Task 2 is itself the deliverable of that exploration step — replaced by the implementer.
- [ ] **Type consistency:** `AuditTimings` field names (`t_received_ns`, `t_policy_done_ns`, `t_impl_start_ns`, `t_impl_done_ns`, `t_audit_written_ns`) used identically across Rust (Task 3), the TypeScript interface (Task 6), and the aggregator (Task 7). The four phase delta names (`rmcp`, `policy`, `impl`, `audit`) match across Task 7 (`phases_p95_ms` keys) and Task 8 (`renderReport` phase table). ✓
