# Study 2 — Adversarial Probe Implementation Plan

> Historical planning artifact — captures the plan as written on the date in the filename; the shipped implementation is the source of truth (see the ADRs and the design spec).

**Goal:** Build a Python harness under `harness/` that runs 4 hand-built adversarial fixtures against two arms — kde-mcp's accessibility-tree-first surface (arm A) and Anthropic's `computer_20251124` tool bridged to kde-mcp's primitives (arm B) — then classify each (scenario × arm) outcome to inform the next-investment decision (vision-augmented mode vs new tools vs schema tuning).

**Architecture:** Standalone Python package outside the Rust workspace (`harness/pyproject.toml`). Anthropic SDK first-class, non-streaming. Both arms drive the same kde-mcp daemon at `http://127.0.0.1:7575/mcp` (deliberate methodology choice — see spec §4.6). Vision arm bridges the schema-less `computer_20251124` actions to kde-mcp's screenshot/mouse/keyboard via a translation table. n=2 per (scenario × arm), tiebreak n=3 only on pass/fail disagreement. Diagnosis drafted in an isolated session loaded with the JSONL logs and the taxonomy excerpt only.

**Tech Stack:** Python 3.11+, `anthropic>=0.45`, `httpx`, `pyyaml`, `PySide6`. `pytest` + `pytest-httpx` for tests. Anthropic beta header `computer-use-2025-11-24` for arm B. Model: `claude-opus-4-7` pinned.

**Budget:** ~$10–18 against a configurable USD cap. Tracked live via `response.usage` × published rates.

**Spec reference:** [`docs/specs/2026-05-19-evaluation-and-latency-design.md`](../specs/2026-05-19-evaluation-and-latency-design.md) §4 (entire), §5 (cost projection).

**Plan amendment convention:** Expect ~30% of commits to be plan syncs ("docs(plans): sync Task N with ...") when an implementing subagent discovers a constraint the planner missed. This is normal — Plan 1 had four such commits and the pattern caught bugs early. Commit the amendment, then continue.

**Cross-cutting-review discipline:** Per [[feedback-cross-cutting-review-catches-contract-bugs]], review every Python ↔ Rust boundary (JSONL format, MCP wire format, bridge action shape) **before** the boundary's first end-to-end run, not just at the end. Phases D, E, and G each close with a cross-cutting review at the boundary.

**Gate-first ordering:** Phase C is the `computer-use-2025-11-24` smoke gate and runs as the first non-carry-forward task (Task 5). Phases D through H are all downstream of it — if the smoke fails (`permission_denied`, `text-only`, transient), do not author them. Phase A's carry-forward fixes (Tasks 1-3) and Phase B's minimal package scaffold (Task 4) ship before the gate because they have independent value: Phase A is unrelated Plan-1 leftovers, and Task 4 is the package skeleton the smoke needs to live in.

---

## File Structure

**Phase A — pre-existing files, carry-forward fixes:**

Modified:
- `src/server.rs` (Task 1) — extract `TimingState` helper; both dispatchers reference it
- `src/audit.rs` (Task 3) — read `KDE_MCP_SESSION_TAG` env at construction; add field to record
- `tools/latency-analyzer/src/cli.ts` (Task 2) — endingLabel from data, not `new Date()`
- `tools/latency-analyzer/src/report.ts` (Task 2 — signature unchanged; tests retargeted)
- `tools/latency-analyzer/test/cli.test.ts` (Task 2) — fixture covers max-ts derivation

**Phase B–G — new harness tree:**

Created:
- `harness/pyproject.toml`
- `harness/README.md`
- `harness/.gitignore` (excludes `runs/`, `.venv/`, `__pycache__/`, `*.egg-info/`)
- `harness/src/harness/__init__.py`
- `harness/src/harness/pricing.py` — per-model token rates
- `harness/src/harness/logger.py` — per-run JSONL logger
- `harness/src/harness/scenario.py` — YAML loader + setup/verify/teardown protocol
- `harness/src/harness/smoke.py` — Phase 0 smoke test (Phase C — gates Phases D-H)
- `harness/src/harness/mcp_client.py` — httpx wrapper, JSON-RPC over `/mcp`
- `harness/src/harness/loop.py` — shared agentic loop
- `harness/src/harness/arms/__init__.py`
- `harness/src/harness/arms/kde_mcp_arm.py`
- `harness/src/harness/arms/vision_arm.py`
- `harness/src/harness/cli.py` — `smoke`, `setup-policy`, `run`, `run-all`, `report`
- `harness/src/harness/report.py` — emit `runs/<ts>/summary.md`
- `harness/tests/test_pricing.py`
- `harness/tests/test_logger.py`
- `harness/tests/test_scenario.py`
- `harness/tests/test_mcp_client.py`
- `harness/tests/test_loop.py`
- `harness/tests/test_kde_mcp_arm.py`
- `harness/tests/test_vision_arm.py`
- `harness/tests/test_cli.py`
- `harness/tests/test_report.py`
- `harness/fixtures/probe-canvas.html` (Task 13)
- `harness/fixtures/probe-qt-noname.py` (Task 14)
- `harness/fixtures/probe-drag.py` (Task 15)
- `harness/fixtures/probe-chart.png` + `harness/fixtures/probe-chart.gen.py` (Task 16)
- `harness/scenarios/E.yaml`, `Q.yaml`, `D.yaml`, `R.yaml`

**Phase H — outputs (gitignored except summary):**

- `harness/runs/<timestamp>/*.jsonl` (gitignored)
- `harness/runs/<timestamp>/spend.json` (gitignored)
- `harness/runs/<timestamp>/summary.md` (committed to `docs/` after curation)
- `docs/adversarial-probe-2026-05-XX.md` (final committed deliverable; Task 19/20)

**Out of scope of this plan:**

- Statistical-n A/B (deferred per spec §6)
- Real-app scenarios (deferred per spec §6)
- LLM-as-judge classification (rejected per spec §4.11)
- CI integration (rejected per spec §6)
- Streaming-mode API (rejected per spec §6)

---

## Phase A — Carry-forward fixes from Plan 1

Three small commits that close out lingering items from Plan 1's final cross-cutting review. Each is independent; ship sequentially on the branch.

### Task 1: Extract `TimingState` helper in `src/server.rs`

`dispatch_audited` (lines 469-607) and `dispatch_audited_with_content` (lines 618-743) share ~130 lines of timing capture, lease handling, and `AuditRecord` assembly. Extract the common pieces into a `TimingState` helper plus a small "build the record" function so both dispatchers reference one copy of the lifecycle logic.

**Files:**
- Modify: `src/server.rs`

- [ ] **Step 1: Read the current shape of both dispatchers**

Run: `sed -n '460,745p' src/server.rs | wc -l`
Expected: ~285 lines of duplicated structure.

Identify the shared sub-steps:
1. `let request_start = std::time::Instant::now()` and `t_received_ns: u64 = 0`
2. Lease-acquire match with the lease-refused early-return path (identical except the success construction differs in payload type)
3. `policy_impl_boundary_ns` single-capture line
4. `t_impl_done_ns` capture after `fut.await`
5. Lease-release on cancel
6. `t_audit_written_ns` capture immediately before `audit.write(rec).await`
7. Building the `AuditRecord` (only the `Content` shape differs between the two)

- [ ] **Step 2: Write a failing test that pins the shared invariant**

Append to the `#[cfg(test)] mod tests` block in `src/server.rs`:

```rust
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn both_dispatchers_emit_identically_shaped_timings() {
        // Run dispatch_audited with a trivial Ok payload and
        // dispatch_audited_with_content with a trivial Vec<Content>.
        // Read the last two AuditRecords from the audit log; assert
        // timings.is_some() and that t_received_ns < t_impl_start_ns
        // <= t_impl_done_ns < t_audit_written_ns on both records.
        let _env = ENV_LOCK.lock().unwrap();
        let (_dir, audit) = fresh_audit().await;
        let path = _dir.path().join("a.jsonl");
        let xdg = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", xdg.path()) };
        let cancel = tokio_util::sync::CancellationToken::new();
        let tray = Arc::new(crate::tray::TrayState::new(cancel.clone(), None));
        let lease = Arc::new(crate::lease::LeaseStore::new_for_test());

        // (1) typed-payload dispatcher
        let _ = dispatch_audited(
            audit.clone(), tray.clone(), lease.clone(),
            crate::lease::ToolKind::Observe,
            "live", None, "test_a", serde_json::Value::Null,
            async { Ok::<_, ToolError>(serde_json::json!({"ok": true})) },
        ).await;

        // (2) content-payload dispatcher
        let _ = dispatch_audited_with_content(
            audit.clone(), tray.clone(), lease.clone(),
            crate::lease::ToolKind::Observe,
            "live", None, "test_b", serde_json::Value::Null,
            async { Ok::<_, ToolError>(vec![Content::text("hello")]) },
        ).await;

        // Poll for two records (audit flush is async)
        let records = read_two_records(&path).await;
        for rec in &records {
            let t = rec.timings.as_ref().expect("timings present");
            assert!(t.t_policy_done_ns >= t.t_received_ns);
            assert!(t.t_impl_start_ns >= t.t_policy_done_ns);
            assert!(t.t_impl_done_ns >= t.t_impl_start_ns);
            assert!(t.t_audit_written_ns >= t.t_impl_done_ns);
        }
        assert_eq!(records[0].tool, "test_a");
        assert_eq!(records[1].tool, "test_b");
    }

    async fn read_two_records(path: &std::path::Path) -> Vec<crate::audit::AuditRecord> {
        for _ in 0..50 {
            let body = tokio::fs::read_to_string(path).await.unwrap_or_default();
            let lines: Vec<_> = body.lines().filter(|l| !l.is_empty()).collect();
            if lines.len() >= 2 {
                return lines.iter()
                    .map(|l| serde_json::from_str::<crate::audit::AuditRecord>(l).unwrap())
                    .filter(|r| r.tool == "test_a" || r.tool == "test_b")
                    .collect();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("audit records not flushed within 1s");
    }
```

(If `crate::lease::LeaseStore::new_for_test()` doesn't exist, mirror the bootstrap used by the existing `dispatch_audited_writes_a_record_on_success` test at the bottom of `src/server.rs`.)

- [ ] **Step 3: Run the test — expect pass (baseline)**

Run: `cargo test --lib server::tests::both_dispatchers_emit_identically_shaped_timings -- --nocapture`
Expected: PASS. (We're capturing the current behaviour before refactoring; both dispatchers already emit timings.)

- [ ] **Step 4: Extract the `TimingState` helper**

Add above `dispatch_audited` in `src/server.rs`:

```rust
/// Shared phase-timing state for both dispatchers.
///
/// Two dispatchers (`dispatch_audited`, `dispatch_audited_with_content`)
/// follow the same five-phase lifecycle — they only differ in the
/// `CallToolResult` payload shape (typed-serialise vs `Vec<Content>`).
/// `TimingState` captures the lifecycle so the two payload paths can
/// share one copy of the boundary captures.
struct TimingState {
    request_start: std::time::Instant,
    t_received_ns: u64,
    t_policy_done_ns: u64,
    t_impl_start_ns: u64,
    t_impl_done_ns: u64,
}

impl TimingState {
    fn start() -> Self {
        Self {
            request_start: std::time::Instant::now(),
            t_received_ns: 0,
            t_policy_done_ns: 0,
            t_impl_start_ns: 0,
            t_impl_done_ns: 0,
        }
    }

    /// Capture the policy-done / impl-start boundary (they collapse onto
    /// the same instant — no work happens between them in either path).
    fn mark_policy_impl_boundary(&mut self) {
        let ns = self.request_start.elapsed().as_nanos() as u64;
        self.t_policy_done_ns = ns;
        self.t_impl_start_ns = ns;
    }

    /// Capture impl-done immediately after the tool future returns.
    fn mark_impl_done(&mut self) {
        self.t_impl_done_ns = self.request_start.elapsed().as_nanos() as u64;
    }

    /// Finalize into an `AuditTimings`. Captures `t_audit_written_ns`
    /// just before this returns — caller must call this immediately
    /// before `audit.write(rec).await` so the timing field reflects
    /// the boundary the spec §3.1 intends.
    fn finish(self) -> crate::audit::AuditTimings {
        let t_audit_written_ns = self.request_start.elapsed().as_nanos() as u64;
        crate::audit::AuditTimings {
            t_received_ns: self.t_received_ns,
            t_policy_done_ns: self.t_policy_done_ns,
            t_impl_start_ns: self.t_impl_start_ns,
            t_impl_done_ns: self.t_impl_done_ns,
            t_audit_written_ns,
        }
    }

    /// Lease-refused early-return: policy/impl collapse onto the
    /// instant of refusal, impl-done equals impl-start.
    fn collapse_for_lease_refused(&mut self) {
        let ns = self.request_start.elapsed().as_nanos() as u64;
        self.t_policy_done_ns = ns;
        self.t_impl_start_ns = ns;
        self.t_impl_done_ns = ns;
    }
}
```

- [ ] **Step 5: Refactor `dispatch_audited` to use `TimingState`**

Replace the body of `dispatch_audited` (currently lines 484-606) with a version that:
1. Calls `let mut ts = TimingState::start();` at entry.
2. In the lease-refused branch, calls `ts.collapse_for_lease_refused();` then `let timings = Some(ts.finish());` immediately before constructing the early-return `AuditRecord`.
3. After the tray-guard line, calls `ts.mark_policy_impl_boundary();`.
4. After `fut.await`, calls `ts.mark_impl_done();`.
5. Just before `audit.write(rec).await`, calls `let timings = Some(ts.finish());` and uses that in the record.

Diff sketch (apply by hand; the function is too long to paste in full here):

```rust
// AT THE TOP of dispatch_audited (replace lines 487-488):
let mut ts = TimingState::start();

// IN THE LEASE-REFUSED BRANCH (replace the timings construction at lines 504-513):
ts.collapse_for_lease_refused();
let timings = Some(ts.finish());

// AT THE POLICY/IMPL BOUNDARY (replace lines 546-548):
ts.mark_policy_impl_boundary();

// AFTER fut.await (replace line 552):
ts.mark_impl_done();

// JUST BEFORE audit.write(rec).await (replace lines 580-590):
let timings = Some(ts.finish());
```

The `AuditRecord` constructor uses the resulting `timings` directly. The five raw `t_*_ns: u64` locals are gone.

- [ ] **Step 6: Apply the same refactor to `dispatch_audited_with_content`**

Same five edit sites, same `TimingState` pattern. After this step both dispatchers share the timing logic.

- [ ] **Step 7: Run the test from Step 2 — expect still PASS**

Run: `cargo test --lib server::tests::both_dispatchers_emit_identically_shaped_timings`
Expected: PASS. The behavioural invariant is preserved.

- [ ] **Step 8: Run the full suite + clippy + fmt**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add src/server.rs
git commit -m "refactor(server): extract TimingState helper for dispatchers

dispatch_audited and dispatch_audited_with_content shared ~130 lines
of timing capture + lease handling + audit-record assembly. The body
of TimingState captures the five-phase lifecycle once; both
dispatchers now consume it. Behaviour-preserving — a new test in
src/server.rs pins the shared invariant.

Cleanup carried forward from Plan 1's final cross-cutting review
(Important #2). Per spec docs/specs/2026-05-19-evaluation-and-latency-design.md
§3.1."
```

---

### Task 2: `latency-analyzer` endingLabel from data

`tools/latency-analyzer/src/cli.ts:56` derives `endingLabel` as `new Date().toISOString().slice(0, 10)` — today's date. That means re-running the analyzer the next day against the same data emits a different `endingLabel`, breaking artifact reproducibility. Change it to `max(r.ts)` over the records actually included in the report.

**Files:**
- Modify: `tools/latency-analyzer/src/cli.ts`
- Modify: `tools/latency-analyzer/test/cli.test.ts`

- [ ] **Step 1: Write the failing test**

Append to `tools/latency-analyzer/test/cli.test.ts`:

```typescript
import { spawn } from "node:child_process";
import { writeFile, mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";

test("CLI endingLabel reflects the max ts of processed records", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "lat-end-"));
  const audit = path.join(dir, "audit.jsonl");
  const rec = (ts: string) => JSON.stringify({
    tool: "observe", ts, ok: true,
    timings: { t_received_ns: 0, t_policy_done_ns: 1e6, t_impl_start_ns: 1.1e6,
               t_impl_done_ns: 2e7, t_audit_written_ns: 2.05e7 },
  });
  await writeFile(audit, [
    rec("2024-01-01T00:00:00Z"),  // a clearly historical date
    rec("2024-01-03T12:00:00Z"),  // ← should be picked as endingLabel
    rec("2024-01-02T00:00:00Z"),
  ].join("\n") + "\n");

  const cliPath = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..", "src", "cli.ts"
  );
  const out = await new Promise<string>((resolve, reject) => {
    const child = spawn("node", ["--import", "tsx", cliPath,
      "--since", "all", "--audit", audit]);
    let stdout = "";
    child.stdout.on("data", (b) => { stdout += b.toString(); });
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code !== 0) reject(new Error(`exit ${code}`));
      else resolve(stdout);
    });
  });

  // Must contain the max-ts date, not today's date.
  assert.match(out, /ending 2024-01-03/);
  assert.doesNotMatch(out, new RegExp(`ending ${new Date().toISOString().slice(0, 10)}`));
});
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd tools/latency-analyzer && npm test -- --test-name-pattern="endingLabel reflects"`
Expected: FAIL — current code emits today's date.

- [ ] **Step 3: Implement — track max(ts) during the filter loop**

In `tools/latency-analyzer/src/cli.ts`, replace the `main()` body around lines 43-58:

```typescript
async function main() {
  const args = parseArgs(process.argv.slice(2));
  const cutoff = Date.now() - (args.sinceMs === Number.POSITIVE_INFINITY
    ? Number.POSITIVE_INFINITY : args.sinceMs);

  const records: AuditRecord[] = [];
  let maxTs: number = Number.NEGATIVE_INFINITY;
  for await (const r of parseJsonl(args.auditPath)) {
    const t = Date.parse(r.ts);
    if (!Number.isFinite(t) || t >= cutoff) {
      records.push(r);
      if (Number.isFinite(t) && t > maxTs) maxTs = t;
    }
  }

  const stats = aggregate(records);
  const sinceLabel =
    args.sinceMs === Number.POSITIVE_INFINITY ? "all time" : describeMs(args.sinceMs);
  // endingLabel reflects the data, not the wall clock. Falling back to
  // today's date only when no record carried a valid ts (which means the
  // analyzer's input was malformed enough that the label is academic).
  const endingLabel = Number.isFinite(maxTs)
    ? new Date(maxTs).toISOString().slice(0, 10)
    : new Date().toISOString().slice(0, 10);
  process.stdout.write(renderReport(stats, { sinceLabel, endingLabel }));
}
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd tools/latency-analyzer && npm test`
Expected: all tests pass including the new one.

- [ ] **Step 5: Verify the typecheck**

Run: `cd tools/latency-analyzer && npx tsc --noEmit`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add tools/latency-analyzer/src/cli.ts tools/latency-analyzer/test/cli.test.ts
git commit -m "fix(latency-analyzer): endingLabel from data, not wall clock

Previously endingLabel was new Date().toISOString().slice(0, 10) —
re-running against the same audit log on a different day produced
a different label, breaking artifact reproducibility. Now derived
from max(r.ts) over the records included in the report.

Cleanup carried forward from Plan 1's final cross-cutting review
(Important #3)."
```

---

### Task 3: `KDE_MCP_SESSION_TAG` audit env support

Spec §4.13 row 10: the harness needs a way to tag its runs so they're distinguishable from normal kde-mcp use in the audit log. Read `KDE_MCP_SESSION_TAG` at audit-log construction (process startup); record on each `AuditRecord` if set.

**Files:**
- Modify: `src/audit.rs`
- Modify: `src/server.rs` (audit-record construction sites — both dispatchers)

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block at the bottom of `src/audit.rs`:

```rust
    #[test]
    fn audit_record_carries_session_tag_when_env_set() {
        // SAFETY: this test mutates a process-global env var. The whole
        // test module already uses ENV_LOCK; if your module has one,
        // hold it. Otherwise, accept that this test must not run in
        // parallel with others touching this env.
        unsafe { std::env::set_var("KDE_MCP_SESSION_TAG", "harness-test-tag"); }
        let tag = read_session_tag();
        assert_eq!(tag.as_deref(), Some("harness-test-tag"));
        unsafe { std::env::remove_var("KDE_MCP_SESSION_TAG"); }
        assert!(read_session_tag().is_none());
    }
```

(If `read_session_tag()` doesn't exist yet, this test fails to compile — that's the failing-test signal.)

- [ ] **Step 2: Run the test — expect failure (compile error)**

Run: `cargo test --lib audit::tests::audit_record_carries_session_tag_when_env_set`
Expected: FAIL with `cannot find function read_session_tag` or similar.

- [ ] **Step 3: Add the env reader + record field**

In `src/audit.rs`:

```rust
/// Reads `KDE_MCP_SESSION_TAG` from the process env. Returns `None` if
/// unset or empty. Used by the harness (and any other framework driving
/// kde-mcp programmatically) to mark its runs so they can be filtered
/// out of normal-use audit-log analysis.
///
/// Spec docs/specs/2026-05-19-evaluation-and-latency-design.md §4.13 row 10.
pub fn read_session_tag() -> Option<String> {
    std::env::var("KDE_MCP_SESSION_TAG").ok().filter(|s| !s.is_empty())
}
```

Add to the `AuditRecord` struct (after `session_id`, before `timings`):

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_tag: Option<String>,
```

Update `default_for_test()` to include `session_tag: None`.

Update every exhaustive `AuditRecord { ... }` construction in `src/audit.rs` to add `session_tag: None` (use `grep -n "AuditRecord {" src/audit.rs` to enumerate sites).

- [ ] **Step 4: Wire the field in both dispatchers**

In `src/server.rs`, at the four `AuditRecord { ... }` sites in `dispatch_audited` and `dispatch_audited_with_content`, add `session_tag: crate::audit::read_session_tag()` immediately before the `timings` field. The env read is cheap (a syscall on Linux), but if it shows up in latency profiles we can cache it at daemon startup later.

- [ ] **Step 5: Run the test — expect pass**

Run: `cargo test --lib audit::tests::audit_record_carries_session_tag_when_env_set`
Expected: PASS.

- [ ] **Step 6: Run the full suite + clippy + fmt**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/audit.rs src/server.rs
git commit -m "feat(audit): record KDE_MCP_SESSION_TAG when set

Optional session_tag field on AuditRecord, read from the process env
at every dispatch. Lets the upcoming evaluation harness mark its runs
so they're distinguishable from normal kde-mcp use.

Per spec docs/specs/2026-05-19-evaluation-and-latency-design.md §4.13 row 10."
```

---

## Phase B — Minimal scaffold (pre-gate)

Four tasks. Set up the Python package, the pricing module (for live cost tracking), the JSONL logger (for one log per run), and the scenario loader (the abstract setup/verify/teardown protocol that the four fixtures will implement).

### Task 4: Python package scaffold

**Files:**
- Create: `harness/pyproject.toml`
- Create: `harness/README.md`
- Create: `harness/.gitignore`
- Create: `harness/src/harness/__init__.py`
- Create: `harness/tests/__init__.py`
- Create: `harness/tests/conftest.py`

- [ ] **Step 1: Create `harness/pyproject.toml`**

```toml
[project]
name = "kde-mcp-harness"
version = "0.1.0"
description = "Adversarial-probe harness for kde-mcp (Study 2). Research-grade — not for redistribution."
requires-python = ">=3.11"
dependencies = [
  "anthropic>=0.45,<1.0",
  "httpx>=0.27,<1.0",
  "pyyaml>=6.0,<7.0",
  "PySide6>=6.6,<7.0",
]

[project.optional-dependencies]
dev = [
  "pytest>=8.0,<9.0",
  "pytest-httpx>=0.32,<1.0",
  "pytest-asyncio>=0.23,<1.0",
]

[project.scripts]
harness = "harness.cli:main"

[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"

[tool.setuptools.packages.find]
where = ["src"]

[tool.pytest.ini_options]
testpaths = ["tests"]
asyncio_mode = "auto"
```

- [ ] **Step 2: Create `harness/README.md`**

```markdown
# kde-mcp adversarial-probe harness

Research-grade harness for Study 2 of the kde-mcp evaluation
(`docs/specs/2026-05-19-evaluation-and-latency-design.md` §4). Runs 4
hand-built fixtures against two arms (kde-mcp's 12 tools vs Anthropic's
`computer_20251124`) and emits a classification per (scenario × arm × trial).

## Install

    cd harness
    python -m venv .venv
    .venv/bin/pip install -e '.[dev]'

## Prerequisites

- Anthropic API key in `ANTHROPIC_API_KEY` with the
  `computer-use-2025-11-24` beta enabled.
- kde-mcp daemon running and reachable at `http://127.0.0.1:7575/mcp`
  (check `curl http://127.0.0.1:7575/healthz`).
- `brave` installed (for fixture E) and `gwenview` installed (for
  fixture R). Both are standard KDE/AUR packages.
- `~/.config/kde-mcp/policy.toml` must auto-approve the four fixture
  classes — see *Policy curation* below.

## Phase 0 smoke (gates everything)

    .venv/bin/harness smoke

Confirms the API key is set, the `computer_20251124` tool is granted,
and a tool-use round-trip works. Cost: ~$0.01. If this fails, do not
proceed.

## Policy curation (one-off)

The fixtures set their KWin app-class explicitly so they can be added
to `policy.toml` for unattended runs:

| Code | Fixture class                  |
|------|---------------------------------|
| E    | kde-mcp-probe-canvas           |
| Q    | kde-mcp-probe-qt-noname        |
| D    | kde-mcp-probe-drag             |
| R    | org.kde.gwenview               |

Run `harness setup-policy` once to launch each fixture, query its
actual KWin class via kde-mcp's `window_query`, and print a YAML
snippet to paste into `~/.config/kde-mcp/policy.toml`. Suggested tier
for harness use: `full` (all actions allowed without per-call approval).

## Run all scenarios

    .venv/bin/harness run-all

Drives 4 scenarios × 2 arms × 2 trials = 16 runs serially. Tiebreaker
trials (up to 4 more) fire only when trials disagree on pass/fail.
Total budget: ~$10–18. Aborts at 85% of `BUDGET_SESSION_USD` (default
$30, override via env).

## Outputs

Each run produces one JSONL at `runs/<timestamp>/<scenario>-<arm>-<trial>.jsonl`.
Run-session aggregate at `runs/<timestamp>/summary.md` and
`runs/<timestamp>/spend.json`. The `runs/` directory is gitignored —
commit `summary.md` manually to `docs/` once the diagnosis is drafted.

## Diagnosis (after runs)

Per spec §4.11, draft the diagnosis in an isolated session
loaded with **only** the JSONL logs from one run-session and the
§4.10 taxonomy excerpt — NOT this design doc, the scenario YAMLs, or
the harness source. Every claim in the diagnosis must quote a specific
event id from the logs.
```

- [ ] **Step 3: Create `harness/.gitignore`**

```
.venv/
__pycache__/
*.pyc
*.egg-info/
.pytest_cache/
runs/
.mypy_cache/
.ruff_cache/
```

- [ ] **Step 4: Create `harness/src/harness/__init__.py`** (empty file, marks the package)

- [ ] **Step 5: Create `harness/tests/__init__.py`** (empty file)

- [ ] **Step 6: Create `harness/tests/conftest.py`**

```python
"""Pytest configuration shared across the harness test suite."""
import os
import pytest


@pytest.fixture
def isolated_env(monkeypatch):
    """Strip env vars that could leak into tests (API keys, session tags)."""
    for var in ("ANTHROPIC_API_KEY", "KDE_MCP_SESSION_TAG",
                "BUDGET_SESSION_USD"):
        monkeypatch.delenv(var, raising=False)
    yield monkeypatch
```

- [ ] **Step 7: Install and verify**

Run:
```bash
cd harness
python -m venv .venv
.venv/bin/pip install -e '.[dev]'
.venv/bin/pytest -q
```

Expected: pip installs cleanly; pytest reports `no tests ran` (empty suite).

- [ ] **Step 8: Commit**

```bash
git add harness/pyproject.toml harness/README.md harness/.gitignore \
        harness/src/harness/__init__.py harness/tests/__init__.py \
        harness/tests/conftest.py
git commit -m "feat(harness): scaffold Python package for Study 2

Standalone package outside the Rust workspace; installable via
'pip install -e harness/[dev]'. Drives kde-mcp at 127.0.0.1:7575
and the Anthropic API directly. Per spec
docs/specs/2026-05-19-evaluation-and-latency-design.md §4.3."
```

---

## Phase C — Phase 0 smoke gate

**Critical:** this phase blocks Phases D through H. If the smoke fails, do not author the rest of the harness — the user's account doesn't have the `computer-use-2025-11-24` beta and the vision arm is non-viable. The user must escalate to Anthropic support or pick a different model before proceeding. The pricing module, logger, scenario loader, MCP client, agentic loop, both arms, all four fixtures, the CLI, and the summary emitter are all downstream of this gate — building any of them before the smoke passes risks throwing away the work.

### Task 5: Phase 0 smoke test

A ~80-line standalone module that verifies the `computer_20251124` tool is granted, the beta header is honoured, and the tool-use round-trip works.

**Files:**
- Create: `harness/src/harness/smoke.py`
- Create: `harness/tests/test_smoke.py`

- [ ] **Step 1: Write the failing test (covers the offline shape only)**

`harness/tests/test_smoke.py`:

```python
"""Tests for the Phase 0 smoke. Live API call is gated behind
HARNESS_LIVE_SMOKE=1; offline tests cover the parsing/classification
of various failure modes."""
import pytest
from harness.smoke import classify_smoke_result, SmokeResult


def test_classify_text_only_response_is_failure():
    """If Claude returns only text and no tool_use, the beta header
    was likely accepted but the model decided not to call the tool —
    not a definitive grant, surface for human review."""
    fake_response = type("R", (), {
        "stop_reason": "end_turn",
        "content": [type("B", (), {"type": "text", "text": "I'd take a screenshot."})()],
    })()
    result = classify_smoke_result(fake_response)
    assert result.status == "text-only"
    assert not result.ok


def test_classify_tool_use_response_is_success():
    fake_response = type("R", (), {
        "stop_reason": "tool_use",
        "content": [
            type("B", (), {"type": "text", "text": "Taking a screenshot."})(),
            type("B", (), {"type": "tool_use", "name": "computer",
                           "input": {"action": "screenshot"}})(),
        ],
    })()
    result = classify_smoke_result(fake_response)
    assert result.status == "ok"
    assert result.ok


def test_smoke_result_is_dataclass_like():
    r = SmokeResult(ok=True, status="ok", detail="")
    assert r.ok
    assert r.status == "ok"


@pytest.mark.skipif(
    not __import__("os").environ.get("HARNESS_LIVE_SMOKE"),
    reason="set HARNESS_LIVE_SMOKE=1 for the live Anthropic API call (~$0.01)"
)
def test_live_smoke():
    """Hits the real Anthropic API. Costs ~$0.01."""
    from harness.smoke import run_smoke
    result = run_smoke()
    print(f"\nlive smoke result: {result}")
    assert result.ok, f"smoke failed: {result.status} — {result.detail}"
```

- [ ] **Step 2: Run the (offline) test — expect failure**

Run: `cd harness && .venv/bin/pytest tests/test_smoke.py -v -k "not live"`
Expected: ImportError on `harness.smoke`.

- [ ] **Step 3: Implement `harness/src/harness/smoke.py`**

```python
"""Phase 0 smoke test — verify computer_20251124 is granted.

Standalone — does NOT depend on any other harness module. Run as:

    .venv/bin/harness smoke

or directly:

    .venv/bin/python -m harness.smoke

Failure modes:
- `no-api-key` — ANTHROPIC_API_KEY env var unset.
- `permission_denied` / `model_not_granted` — the account/key
  doesn't yet include the computer-use beta. Surface to user;
  do NOT proceed past Phase C.
- `text-only` — Claude returned text instead of a tool_use block.
  Could mean the beta header wasn't honoured, or the prompt is
  too weak. Surface; do NOT proceed without escalation.
- `transient` — network or 5xx. Retry once; if still failing,
  surface; do NOT proceed.

Cost: ~$0.01 per invocation. Run before authoring the rest of
the harness.
"""
from __future__ import annotations
import base64
import os
from dataclasses import dataclass


# A 1×1 transparent PNG, base64-encoded. Used as the stub screenshot
# in the round-trip so we don't need a real grab.
_BLANK_PNG_B64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII="
)


@dataclass
class SmokeResult:
    ok: bool
    status: str  # "ok" | "no-api-key" | "permission_denied" | "text-only" | "transient" | "other"
    detail: str = ""


def classify_smoke_result(response) -> SmokeResult:
    """Parse a successful Anthropic response into a SmokeResult.

    A successful smoke is `stop_reason == "tool_use"` with at least one
    `tool_use` content block. Anything else is suspect.
    """
    content = getattr(response, "content", [])
    has_tool_use = any(getattr(b, "type", None) == "tool_use" for b in content)
    if has_tool_use:
        return SmokeResult(ok=True, status="ok",
                           detail=f"stop_reason={response.stop_reason}")
    return SmokeResult(
        ok=False, status="text-only",
        detail=f"stop_reason={response.stop_reason}; no tool_use block",
    )


def run_smoke() -> SmokeResult:
    """Execute the live smoke. Returns a SmokeResult."""
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        return SmokeResult(ok=False, status="no-api-key",
                           detail="set ANTHROPIC_API_KEY in env")

    from anthropic import Anthropic, APIStatusError, APIConnectionError
    client = Anthropic(api_key=api_key)

    try:
        # Step 1: ask Claude to take a screenshot.
        resp = client.beta.messages.create(
            model="claude-opus-4-7",
            max_tokens=1024,
            tools=[{
                "type": "computer_20251124", "name": "computer",
                "display_width_px": 1920, "display_height_px": 1080,
            }],
            betas=["computer-use-2025-11-24"],
            messages=[{"role": "user", "content": "Take a screenshot."}],
        )
    except APIStatusError as e:
        status = getattr(e, "status_code", 0)
        msg = str(e).lower()
        if "permission" in msg or "not granted" in msg or status == 403:
            return SmokeResult(ok=False, status="permission_denied",
                               detail=str(e)[:300])
        return SmokeResult(ok=False, status="other", detail=str(e)[:300])
    except APIConnectionError as e:
        return SmokeResult(ok=False, status="transient", detail=str(e)[:300])

    return classify_smoke_result(resp)


def main() -> int:
    result = run_smoke()
    print(f"smoke: {result.status}")
    if result.detail:
        print(f"  detail: {result.detail}")
    print(f"  ok: {result.ok}")
    return 0 if result.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Run the offline test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_smoke.py -v -k "not live"`
Expected: 3 offline tests pass.

- [ ] **Step 5: Run the LIVE smoke against the real API**

```bash
cd harness
HARNESS_LIVE_SMOKE=1 .venv/bin/pytest tests/test_smoke.py::test_live_smoke -v -s
```

Expected: PASS — Claude requests `screenshot` via the `computer` tool. The pytest captured stdout (`-s`) prints `live smoke result: SmokeResult(ok=True, status='ok', ...)`. Cost ~$0.01.

**If this FAILS:**
- `permission_denied` → Stop. Tell the user. The account/key doesn't yet include the `computer-use-2025-11-24` beta. Do NOT proceed past Phase C.
- `text-only` → Stop. Likely the beta header wasn't honoured. Re-check the SDK version is recent enough (`pip show anthropic`). Surface to user; do NOT proceed.
- `transient` → Retry the command once. If still failing, surface; do NOT proceed.

- [ ] **Step 6: Commit**

```bash
git add harness/src/harness/smoke.py harness/tests/test_smoke.py
git commit -m "feat(harness): Phase 0 smoke test gates the rest of Study 2

Standalone module that verifies computer_20251124 is granted to the
account and tool-use round-trips work. Run before authoring the
agentic loop and arms. Cost ~\$0.01 per invocation. Per spec §4.7."
```

---

## Phase D — Harness scaffolding (post-gate)

Three tasks: pricing module, JSONL logger, scenario loader. All gated by Phase C — building these before the smoke passes risks throwing the work away if the vision arm turns out to be non-viable. Cross-cutting reviewer fires at the end of this phase.

### Task 6: Pricing module

Live cost tracking off `response.usage` fields. Per the spec's $0.50 per-trial cap and $30 per-session abort threshold.

**Files:**
- Create: `harness/src/harness/pricing.py`
- Create: `harness/tests/test_pricing.py`

- [ ] **Step 1: Write the failing test**

`harness/tests/test_pricing.py`:

```python
"""Tests for the pricing module."""
from harness.pricing import cost_of_usage, RATES, ModelRates


def test_rates_includes_claude_opus_4_7():
    assert "claude-opus-4-7" in RATES
    r = RATES["claude-opus-4-7"]
    assert isinstance(r, ModelRates)
    assert r.input_per_mtok > 0
    assert r.output_per_mtok > r.input_per_mtok  # output always more expensive


def test_cost_of_usage_basic():
    usage = {"input_tokens": 1000, "output_tokens": 500,
             "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    cost = cost_of_usage(usage, model="claude-opus-4-7")
    r = RATES["claude-opus-4-7"]
    expected = (1000 / 1_000_000) * r.input_per_mtok + (500 / 1_000_000) * r.output_per_mtok
    assert abs(cost - expected) < 1e-9


def test_cost_of_usage_with_cache():
    # Cache reads are ~10% of input. Cache creation is 1.25x input.
    usage = {
        "input_tokens": 100,
        "output_tokens": 50,
        "cache_creation_input_tokens": 500,
        "cache_read_input_tokens": 5000,
    }
    cost = cost_of_usage(usage, model="claude-opus-4-7")
    assert cost > 0


def test_unknown_model_raises():
    import pytest
    with pytest.raises(KeyError):
        cost_of_usage({"input_tokens": 1}, model="claude-fictional-1")
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd harness && .venv/bin/pytest tests/test_pricing.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/pricing.py`**

```python
"""Per-model token pricing for cost tracking.

Rates are USD per 1M tokens as published at
https://www.anthropic.com/pricing. Update when prices change.

Cache pricing follows Anthropic's documented formula:
- cache_creation_input_tokens: 1.25 × input rate
- cache_read_input_tokens:     0.10 × input rate

Image tokens are counted as input_tokens by the API (the per-image
token cost is computed server-side and added to input_tokens before
being returned in the response.usage block). No separate image rate.
"""
from dataclasses import dataclass


@dataclass(frozen=True)
class ModelRates:
    input_per_mtok: float
    output_per_mtok: float
    cache_creation_multiplier: float = 1.25
    cache_read_multiplier: float = 0.10


# Pricing as of 2026-05 — see comment at top of file. Verify before
# running a long session if prices have changed.
RATES: dict[str, ModelRates] = {
    "claude-opus-4-7": ModelRates(
        input_per_mtok=15.00,
        output_per_mtok=75.00,
    ),
    "claude-opus-4-6": ModelRates(
        input_per_mtok=15.00,
        output_per_mtok=75.00,
    ),
    "claude-sonnet-4-6": ModelRates(
        input_per_mtok=3.00,
        output_per_mtok=15.00,
    ),
    "claude-haiku-4-5-20251001": ModelRates(
        input_per_mtok=1.00,
        output_per_mtok=5.00,
    ),
}


def cost_of_usage(usage: dict, *, model: str) -> float:
    """Compute USD cost of a single Anthropic response.usage block.

    `usage` is the dict returned by the SDK (keys: input_tokens,
    output_tokens, cache_creation_input_tokens, cache_read_input_tokens).
    Missing fields default to 0.
    """
    r = RATES[model]
    inp = usage.get("input_tokens", 0)
    out = usage.get("output_tokens", 0)
    cache_c = usage.get("cache_creation_input_tokens", 0)
    cache_r = usage.get("cache_read_input_tokens", 0)

    cost = (
        (inp / 1_000_000) * r.input_per_mtok
        + (out / 1_000_000) * r.output_per_mtok
        + (cache_c / 1_000_000) * r.input_per_mtok * r.cache_creation_multiplier
        + (cache_r / 1_000_000) * r.input_per_mtok * r.cache_read_multiplier
    )
    return cost
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_pricing.py -v`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add harness/src/harness/pricing.py harness/tests/test_pricing.py
git commit -m "feat(harness): pricing module for live cost tracking

Anthropic per-model token rates + cache multipliers. cost_of_usage()
takes a response.usage dict and returns USD; sum across steps for
per-trial spend, then enforce the BUDGET_CAP_USD guardrail in the
agentic loop. Per spec §4.13 row 4."
```

---

### Task 7: JSONL logger

One file per (scenario × arm × trial). Event types follow spec §4.9 verbatim — keep field names aligned.

**Files:**
- Create: `harness/src/harness/logger.py`
- Create: `harness/tests/test_logger.py`

- [ ] **Step 1: Write the failing test**

`harness/tests/test_logger.py`:

```python
"""Tests for the JSONL run logger."""
import json
from pathlib import Path
from harness.logger import RunLogger


def test_logger_writes_each_event_as_one_line(tmp_path: Path):
    log = RunLogger(tmp_path / "run.jsonl")
    log.event("run_start", scenario="E", arm="vision", trial=1)
    log.event("step", n=0, claude_text="hello")
    log.close()

    lines = (tmp_path / "run.jsonl").read_text().splitlines()
    assert len(lines) == 2
    first = json.loads(lines[0])
    assert first["event"] == "run_start"
    assert first["scenario"] == "E"
    second = json.loads(lines[1])
    assert second["event"] == "step"
    assert second["n"] == 0


def test_logger_includes_iso_timestamp(tmp_path: Path):
    log = RunLogger(tmp_path / "run.jsonl")
    log.event("step", n=0)
    log.close()
    rec = json.loads((tmp_path / "run.jsonl").read_text().strip())
    # ISO 8601 with Z or +00:00 suffix
    assert "T" in rec["t"]
    assert rec["t"].endswith(("Z", "+00:00"))


def test_logger_flushes_on_each_event(tmp_path: Path):
    # If the harness crashes mid-run, the logs up to the crash must be
    # readable. Verify by writing then reading without close().
    log = RunLogger(tmp_path / "run.jsonl")
    log.event("step", n=0)
    log.event("step", n=1)
    # No close() called — file should still have both lines flushed.
    lines = (tmp_path / "run.jsonl").read_text().splitlines()
    assert len(lines) == 2
    log.close()


def test_logger_event_id_format(tmp_path: Path):
    """Event ids referenced in the diagnosis (e.g. [E-vision-trial1-step-4])
    are stable — the logger emits an `event_id` field per event so the
    diagnosis can quote it verbatim."""
    log = RunLogger(tmp_path / "run.jsonl",
                    scenario="E", arm="vision", trial=1)
    log.event("step", n=4)
    log.close()
    rec = json.loads((tmp_path / "run.jsonl").read_text().strip())
    assert rec["event_id"] == "E-vision-trial1-step-4"


def test_logger_event_id_for_non_step_events(tmp_path: Path):
    log = RunLogger(tmp_path / "run.jsonl",
                    scenario="Q", arm="kde-mcp", trial=2)
    log.event("verify", result="pass")
    log.close()
    rec = json.loads((tmp_path / "run.jsonl").read_text().strip())
    assert rec["event_id"] == "Q-kde-mcp-trial2-verify"
```

- [ ] **Step 2: Run the test — expect failure**

Run: `cd harness && .venv/bin/pytest tests/test_logger.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/logger.py`**

```python
"""Per-run JSONL logger.

One file per (scenario × arm × trial). Each line is one event with an
`event` discriminator, an ISO 8601 `t` field, and an `event_id` of the
form `<scenario>-<arm>-trial<trial>-<event>[-<n>]`. The diagnosis
protocol (spec §4.11) requires every claim to quote an event_id, so
the format is load-bearing — don't change it without updating the
diagnosis runbook.
"""
from __future__ import annotations
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


class RunLogger:
    """Append-flush JSONL logger.

    Flushes after every `event` call so a crash mid-run leaves a
    parseable prefix on disk. Use `with RunLogger(...) as log:` or
    call `close()` explicitly.
    """

    def __init__(
        self,
        path: Path,
        *,
        scenario: str = "",
        arm: str = "",
        trial: int = 0,
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self._path = path
        self._file = path.open("a", encoding="utf-8")
        self._scenario = scenario
        self._arm = arm
        self._trial = trial

    def event(self, event: str, **fields: Any) -> str:
        """Write one event line, return its event_id."""
        event_id = self._make_event_id(event, fields)
        record: dict[str, Any] = {
            "event": event,
            "event_id": event_id,
            "t": datetime.now(timezone.utc).isoformat(),
        }
        record.update(fields)
        self._file.write(json.dumps(record, separators=(",", ":")) + "\n")
        self._file.flush()
        return event_id

    def _make_event_id(self, event: str, fields: dict[str, Any]) -> str:
        # Step events get suffixed by n; others by the event name only.
        if event == "step" and "n" in fields:
            return f"{self._scenario}-{self._arm}-trial{self._trial}-step-{fields['n']}"
        return f"{self._scenario}-{self._arm}-trial{self._trial}-{event}"

    def close(self) -> None:
        if not self._file.closed:
            self._file.close()

    def __enter__(self) -> "RunLogger":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_logger.py -v`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add harness/src/harness/logger.py harness/tests/test_logger.py
git commit -m "feat(harness): JSONL run logger with stable event_ids

One file per (scenario × arm × trial). Event format follows spec §4.9
verbatim; event_id format follows §4.11 diagnosis protocol
(<scenario>-<arm>-trial<trial>-<event>[-n]). Per-event flush so a
crash mid-run leaves a parseable prefix."
```

---

### Task 8: Scenario loader

Generic YAML loader + an abstract `Scenario` protocol with `setup`, `verify`, `teardown` methods. Per-fixture concrete subclasses arrive in Phase F.

**Files:**
- Create: `harness/src/harness/scenario.py`
- Create: `harness/tests/test_scenario.py`
- Create: `harness/tests/fixtures/scenario_E_test.yaml` (test fixture, not the real scenario YAML)

- [ ] **Step 1: Create the test fixture YAML**

`harness/tests/fixtures/scenario_E_test.yaml`:

```yaml
code: E
title: Canvas-digit buttons
model: claude-opus-4-7
prompt: "Click the canvas button showing the number 3."
fixture:
  type: html
  path: fixtures/probe-canvas.html
  app_class: kde-mcp-probe-canvas
verify:
  type: file_contains
  path: /tmp/probe-e.txt
  expect: "3"
budget_usd: 0.50
step_cap: 50
```

- [ ] **Step 2: Write the failing test**

`harness/tests/test_scenario.py`:

```python
"""Tests for the scenario loader."""
from pathlib import Path
import pytest
from harness.scenario import load_scenario, Scenario


FIXTURES = Path(__file__).parent / "fixtures"


def test_load_scenario_parses_E_test_yaml():
    s = load_scenario(FIXTURES / "scenario_E_test.yaml")
    assert isinstance(s, Scenario)
    assert s.code == "E"
    assert s.model == "claude-opus-4-7"
    assert "canvas button" in s.prompt
    assert s.budget_usd == 0.50
    assert s.step_cap == 50


def test_load_scenario_default_step_cap():
    """Step cap defaults to 50 if omitted."""
    p = FIXTURES / "scenario_no_caps.yaml"
    p.write_text(
        "code: X\ntitle: t\nmodel: claude-opus-4-7\n"
        "prompt: p\nfixture:\n  type: html\n  path: x\n"
        "  app_class: x\nverify:\n  type: file_contains\n"
        "  path: /tmp/x\n  expect: y\n"
    )
    try:
        s = load_scenario(p)
        assert s.step_cap == 50
        assert s.budget_usd == 0.50
    finally:
        p.unlink()


def test_load_scenario_rejects_missing_fields():
    bad = FIXTURES / "bad.yaml"
    bad.write_text("code: X\n")  # missing prompt, verify, etc.
    try:
        with pytest.raises((KeyError, ValueError)):
            load_scenario(bad)
    finally:
        bad.unlink()
```

- [ ] **Step 3: Run the test — expect failure**

Run: `cd harness && .venv/bin/pytest tests/test_scenario.py -v`
Expected: ImportError.

- [ ] **Step 4: Implement `harness/src/harness/scenario.py`**

```python
"""Scenario loader + protocol.

A `Scenario` is a fixture launcher plus a verifier plus a prompt. The
YAML carries the static config; the launcher/verifier implementations
live in this module (`HtmlFixtureLauncher`, `PythonFixtureLauncher`,
`ImageFixtureLauncher`, `FileContainsVerifier`, `CoordRangeVerifier`,
`ResponseContainsVerifier`).

A Scenario instance is the runtime object the loop talks to via
`setup()` / `verify(transcript)` / `teardown()`.
"""
from __future__ import annotations
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol
import os
import signal
import subprocess
import time
import yaml


# ─── protocol ─────────────────────────────────────────────────────────

class FixtureLauncher(Protocol):
    """Launches the fixture; returns the PID for teardown."""
    def launch(self, repo_root: Path) -> int: ...
    def app_class(self) -> str: ...


class Verifier(Protocol):
    """Runs after the agent loop completes. Returns (passed, details)."""
    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]: ...


# ─── data class ───────────────────────────────────────────────────────

@dataclass
class Scenario:
    code: str
    title: str
    model: str
    prompt: str
    launcher: FixtureLauncher
    verifier: Verifier
    budget_usd: float = 0.50
    step_cap: int = 50
    _pid: int | None = field(default=None, init=False)

    def setup(self, repo_root: Path) -> None:
        # Kill any leftover process with the same app_class before launching.
        self._kill_existing(self.launcher.app_class())
        self._pid = self.launcher.launch(repo_root)

    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]:
        return self.verifier.verify(transcript)

    def teardown(self) -> None:
        if self._pid:
            try:
                os.kill(self._pid, signal.SIGTERM)
                time.sleep(0.2)
                os.kill(self._pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self._pid = None

    @staticmethod
    def _kill_existing(app_class: str) -> None:
        # `pgrep -f <class>` returns PIDs whose argv contains the class.
        # Best-effort; missing pgrep is fine.
        try:
            out = subprocess.run(
                ["pgrep", "-f", app_class],
                capture_output=True, text=True, timeout=2,
            )
            for pid_str in out.stdout.strip().splitlines():
                try:
                    os.kill(int(pid_str), signal.SIGKILL)
                except (ProcessLookupError, ValueError):
                    pass
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass


# ─── concrete launchers ───────────────────────────────────────────────

@dataclass
class HtmlFixtureLauncher:
    """Launches a static HTML file in Brave with a custom --class."""
    path: str
    _app_class: str

    def app_class(self) -> str:
        return self._app_class

    def launch(self, repo_root: Path) -> int:
        url = (repo_root / self.path).resolve().as_uri()
        proc = subprocess.Popen(
            ["brave", f"--class={self._app_class}", url],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        # Give the window a moment to appear before the loop captures geometry.
        time.sleep(1.5)
        return proc.pid


@dataclass
class PythonFixtureLauncher:
    """Launches a PySide6 fixture by running its script."""
    path: str
    _app_class: str

    def app_class(self) -> str:
        return self._app_class

    def launch(self, repo_root: Path) -> int:
        script = repo_root / self.path
        proc = subprocess.Popen(
            ["python", str(script)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        time.sleep(1.5)
        return proc.pid


@dataclass
class ImageFixtureLauncher:
    """Opens an image file in Gwenview."""
    path: str

    def app_class(self) -> str:
        return "org.kde.gwenview"

    def launch(self, repo_root: Path) -> int:
        img = repo_root / self.path
        proc = subprocess.Popen(
            ["gwenview", str(img)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        time.sleep(1.5)
        return proc.pid


# ─── concrete verifiers ───────────────────────────────────────────────

@dataclass
class FileContainsVerifier:
    path: str
    expect: str

    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]:
        try:
            body = Path(self.path).read_text().strip()
        except FileNotFoundError:
            return False, {"reason": "verify-file-missing", "path": self.path}
        return (self.expect in body, {"actual": body, "expected": self.expect})


@dataclass
class CoordRangeVerifier:
    """The fixture writes its final geometry to a file; check it falls
    within the [lo, hi] range on the named axis."""
    path: str
    axis: str  # "x" | "y"
    lo: float
    hi: float

    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]:
        try:
            body = Path(self.path).read_text().strip()
        except FileNotFoundError:
            return False, {"reason": "verify-file-missing", "path": self.path}
        try:
            x, y = (float(p) for p in body.split(","))
        except ValueError:
            return False, {"reason": "verify-file-malformed", "body": body}
        coord = x if self.axis == "x" else y
        return (self.lo <= coord <= self.hi,
                {"actual": coord, "range": [self.lo, self.hi]})


@dataclass
class ResponseContainsVerifier:
    """Check the LAST assistant message in the transcript for a substring."""
    expect: str

    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]:
        if not transcript:
            return False, {"reason": "empty-transcript"}
        last = transcript[-1]
        return (self.expect in last, {"expected": self.expect, "last": last[:200]})


# ─── loader ───────────────────────────────────────────────────────────

def load_scenario(path: Path) -> Scenario:
    data = yaml.safe_load(path.read_text())
    required = ["code", "title", "model", "prompt", "fixture", "verify"]
    for key in required:
        if key not in data:
            raise KeyError(f"scenario {path} missing required field: {key}")

    launcher = _make_launcher(data["fixture"])
    verifier = _make_verifier(data["verify"])

    return Scenario(
        code=data["code"],
        title=data["title"],
        model=data["model"],
        prompt=data["prompt"],
        launcher=launcher,
        verifier=verifier,
        budget_usd=float(data.get("budget_usd", 0.50)),
        step_cap=int(data.get("step_cap", 50)),
    )


def _make_launcher(spec: dict) -> FixtureLauncher:
    kind = spec["type"]
    if kind == "html":
        return HtmlFixtureLauncher(path=spec["path"], _app_class=spec["app_class"])
    if kind == "python":
        return PythonFixtureLauncher(path=spec["path"], _app_class=spec["app_class"])
    if kind == "image":
        return ImageFixtureLauncher(path=spec["path"])
    raise ValueError(f"unknown fixture type: {kind}")


def _make_verifier(spec: dict) -> Verifier:
    kind = spec["type"]
    if kind == "file_contains":
        return FileContainsVerifier(path=spec["path"], expect=spec["expect"])
    if kind == "coord_range":
        return CoordRangeVerifier(
            path=spec["path"], axis=spec["axis"],
            lo=float(spec["lo"]), hi=float(spec["hi"]),
        )
    if kind == "response_contains":
        return ResponseContainsVerifier(expect=spec["expect"])
    raise ValueError(f"unknown verify type: {kind}")
```

- [ ] **Step 5: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_scenario.py -v`
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add harness/src/harness/scenario.py harness/tests/test_scenario.py harness/tests/fixtures/scenario_E_test.yaml
git commit -m "feat(harness): scenario loader + launcher/verifier protocol

YAML loader for scenarios + concrete HtmlFixtureLauncher,
PythonFixtureLauncher, ImageFixtureLauncher, FileContainsVerifier,
CoordRangeVerifier, ResponseContainsVerifier. Per spec §4.3 layout +
§4.2 verification table."
```

---

## Phase E — Harness core

**Gated by Phase C.** Four tasks: the MCP HTTP client, the shared agentic loop, and the two arm implementations.

### Task 9: MCP HTTP client

Hand-rolled httpx wrapper for kde-mcp's `/mcp` endpoint. Per spec §4.3 — not the official `mcp` Python SDK, to keep dependencies tight. Implements just enough JSON-RPC 2.0 over Streamable HTTP to call `initialize`, `tools/list`, and `tools/call`.

**Files:**
- Create: `harness/src/harness/mcp_client.py`
- Create: `harness/tests/test_mcp_client.py`

- [ ] **Step 1: Write the failing test (offline shape only; live test gated by env)**

`harness/tests/test_mcp_client.py`:

```python
"""Tests for the MCP HTTP client.

Offline tests use pytest-httpx to mock /mcp responses. The live test
(gated behind HARNESS_LIVE_MCP=1) hits the real daemon at
127.0.0.1:7575 and asserts the tools/list response carries kde-mcp's
expected tool names."""
import json
import os
import pytest
import httpx
from harness.mcp_client import MCPClient, MCPError


# pytest-httpx provides the `httpx_mock` fixture.

def test_initialize_sends_correct_jsonrpc(httpx_mock):
    httpx_mock.add_response(
        url="http://127.0.0.1:7575/mcp",
        json={"jsonrpc": "2.0", "id": 1,
              "result": {"protocolVersion": "2025-11-25",
                         "serverInfo": {"name": "kde-mcp", "version": "0.1.0"}}},
    )
    client = MCPClient("http://127.0.0.1:7575/mcp")
    info = client.initialize()
    assert info["serverInfo"]["name"] == "kde-mcp"

    sent = httpx_mock.get_request()
    body = json.loads(sent.read())
    assert body["method"] == "initialize"
    assert body["jsonrpc"] == "2.0"
    # Required headers
    assert "application/json" in sent.headers["accept"]
    assert "text/event-stream" in sent.headers["accept"]


def test_tools_list_returns_tool_array(httpx_mock):
    httpx_mock.add_response(
        url="http://127.0.0.1:7575/mcp",
        json={"jsonrpc": "2.0", "id": 1,
              "result": {"tools": [{"name": "observe", "description": "...",
                                    "inputSchema": {"type": "object"}}]}},
    )
    client = MCPClient("http://127.0.0.1:7575/mcp")
    tools = client.list_tools()
    assert len(tools) == 1
    assert tools[0]["name"] == "observe"


def test_call_tool_propagates_result_payload(httpx_mock):
    httpx_mock.add_response(
        url="http://127.0.0.1:7575/mcp",
        json={"jsonrpc": "2.0", "id": 1,
              "result": {"content": [{"type": "text", "text": '{"ok": true}'}],
                         "isError": False}},
    )
    client = MCPClient("http://127.0.0.1:7575/mcp")
    result = client.call_tool("observe", {})
    assert result["isError"] is False
    assert result["content"][0]["text"] == '{"ok": true}'


def test_call_tool_raises_on_jsonrpc_error(httpx_mock):
    httpx_mock.add_response(
        url="http://127.0.0.1:7575/mcp",
        json={"jsonrpc": "2.0", "id": 1,
              "error": {"code": -32601, "message": "Method not found"}},
    )
    client = MCPClient("http://127.0.0.1:7575/mcp")
    with pytest.raises(MCPError):
        client.call_tool("nonexistent", {})


@pytest.mark.skipif(
    not os.environ.get("HARNESS_LIVE_MCP"),
    reason="set HARNESS_LIVE_MCP=1 to hit the real daemon at 127.0.0.1:7575"
)
def test_live_tools_list_includes_observe():
    client = MCPClient("http://127.0.0.1:7575/mcp")
    client.initialize()
    tools = client.list_tools()
    names = {t["name"] for t in tools}
    # Per ADR-0013 the wire tools include at minimum these:
    expected = {"observe", "act_on_element", "mouse", "keyboard_type",
                "keyboard_key", "screenshot", "read_text", "window_query",
                "window_control", "clipboard_read", "clipboard_write",
                "inspect"}
    assert expected.issubset(names), f"missing tools: {expected - names}"
```

- [ ] **Step 2: Run the offline tests — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_mcp_client.py -v -k "not live"`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/mcp_client.py`**

```python
"""Minimal JSON-RPC 2.0 client for kde-mcp's HTTP MCP endpoint.

kde-mcp exposes Streamable HTTP at /mcp (see
src/transport/daemon.rs). For our purposes — three methods —
JSON-form responses are sufficient; we set Accept to allow both
JSON and SSE, but the daemon's tools/list and tools/call return JSON
for the small payloads we send.

Per ADR-0015 the daemon is unauthenticated on 127.0.0.1, so no
bearer token is needed. Per the [[reference-claude-code-mcp-caveats]]
memory: clients that respect HTTPS_PROXY for loopback will route the
request through the proxy. Set NO_PROXY=127.0.0.1 in the env if you
see unexpected proxy errors. The harness does not configure proxies
itself.
"""
from __future__ import annotations
import itertools
from typing import Any
import httpx


class MCPError(Exception):
    """Raised when the daemon returns a JSON-RPC error object."""
    def __init__(self, code: int, message: str, data: Any = None) -> None:
        super().__init__(f"MCP error {code}: {message}")
        self.code = code
        self.data = data


class MCPClient:
    """One client per logical session. Holds the Mcp-Session-Id header
    after initialize() so subsequent calls land in the same session."""

    def __init__(self, url: str, timeout: float = 30.0) -> None:
        self._url = url
        self._client = httpx.Client(
            timeout=timeout,
            headers={"Accept": "application/json, text/event-stream",
                     "Content-Type": "application/json"},
        )
        self._id_counter = itertools.count(1)
        self._session_id: str | None = None

    def initialize(self) -> dict:
        result = self._call("initialize", {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "kde-mcp-harness", "version": "0.1.0"},
        }, is_initialize=True)
        # Per the MCP spec, the client MUST send `notifications/initialized`
        # after initialize. Streamable HTTP returns 202 with no body.
        self._notify("notifications/initialized")
        return result

    def list_tools(self) -> list[dict]:
        result = self._call("tools/list", {})
        return result.get("tools", [])

    def call_tool(self, name: str, arguments: dict) -> dict:
        return self._call("tools/call", {"name": name, "arguments": arguments})

    def close(self) -> None:
        self._client.close()

    # ── internals ─────────────────────────────────────────────────────

    def _call(self, method: str, params: dict, *, is_initialize: bool = False) -> dict:
        req_id = next(self._id_counter)
        body = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
        headers: dict[str, str] = {}
        if self._session_id and not is_initialize:
            headers["Mcp-Session-Id"] = self._session_id
        resp = self._client.post(self._url, json=body, headers=headers or None)
        resp.raise_for_status()
        if is_initialize:
            sid = resp.headers.get("mcp-session-id") or resp.headers.get("Mcp-Session-Id")
            if sid:
                self._session_id = sid
        envelope = resp.json()
        if "error" in envelope:
            err = envelope["error"]
            raise MCPError(err.get("code", 0), err.get("message", ""), err.get("data"))
        return envelope.get("result", {})

    def _notify(self, method: str, params: dict | None = None) -> None:
        body: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            body["params"] = params
        headers = {"Mcp-Session-Id": self._session_id} if self._session_id else {}
        # Notifications return 202 No Content; do not parse body.
        resp = self._client.post(self._url, json=body, headers=headers or None)
        # 202 (Accepted) or 200 are both fine for notifications.
        if resp.status_code not in (200, 202):
            resp.raise_for_status()
```

- [ ] **Step 4: Run the offline tests — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_mcp_client.py -v -k "not live"`
Expected: 4 offline tests pass.

- [ ] **Step 5: Run the live test against the daemon**

Pre-requisite: kde-mcp daemon is up and serving 127.0.0.1:7575 (`curl http://127.0.0.1:7575/healthz` returns `{"ok": true, ...}`).

```bash
cd harness
HARNESS_LIVE_MCP=1 .venv/bin/pytest tests/test_mcp_client.py::test_live_tools_list_includes_observe -v
```

Expected: PASS. The daemon returns all 12 wire tools.

If this fails because of a wire-format mismatch between our hand-rolled JSON-RPC and rmcp's Streamable HTTP — dispatch a cross-cutting reviewer (per the Plan 1 lesson) before working around the issue. The contract bug we're guarding against is exactly this kind of producer/consumer drift.

- [ ] **Step 6: Commit**

```bash
git add harness/src/harness/mcp_client.py harness/tests/test_mcp_client.py
git commit -m "feat(harness): minimal MCP HTTP client for kde-mcp

Hand-rolled JSON-RPC 2.0 over httpx; just initialize, tools/list,
tools/call. Maintains the Mcp-Session-Id across calls. Offline tests
via pytest-httpx; live test (HARNESS_LIVE_MCP=1) hits the daemon and
asserts kde-mcp's 12 tool names are present. Per spec §4.3."
```

---

### Task 10: Shared agentic loop

The arm-agnostic loop from spec §4.4 pseudocode, made concrete. Caps spend ($BUDGET_CAP_USD per trial) and steps (STEP_RUNAWAY_CAP=50). Non-streaming Anthropic SDK.

**Files:**
- Create: `harness/src/harness/loop.py`
- Create: `harness/tests/test_loop.py`

- [ ] **Step 1: Write the failing test (offline only; uses fake arm)**

`harness/tests/test_loop.py`:

```python
"""Tests for the shared agentic loop.

All offline. The Anthropic SDK is mocked via a fake client; the loop's
contract with the arm interface is exercised end-to-end."""
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from harness.loop import run_loop, LoopResult
from harness.logger import RunLogger
from harness.scenario import Scenario, FileContainsVerifier, HtmlFixtureLauncher


# ── Test doubles ───────────────────────────────────────────────────────

class FakeAnthropicClient:
    """Scripted responses; each call to messages.create pops the next."""
    def __init__(self, scripted_responses: list):
        self._queue = list(scripted_responses)
        self.beta = self  # so `client.beta.messages.create` resolves
        self.messages = self

    def create(self, **kwargs):
        if not self._queue:
            raise AssertionError("loop made an unexpected call to messages.create")
        return self._queue.pop(0)


def fake_resp(stop_reason: str, content_blocks: list, usage: dict | None = None):
    return type("R", (), {
        "stop_reason": stop_reason,
        "content": content_blocks,
        "usage": type("U", (), usage or {"input_tokens": 100, "output_tokens": 50,
                                          "cache_creation_input_tokens": 0,
                                          "cache_read_input_tokens": 0})(),
        "id": "msg_test",
        "model": "claude-opus-4-7",
    })()


def text_block(text):
    return type("B", (), {"type": "text", "text": text})()


def tool_use_block(name, inp, id="t1"):
    return type("B", (), {"type": "tool_use", "name": name, "input": inp, "id": id})()


@dataclass
class FakeArm:
    """Minimal arm that doesn't actually call any tools."""
    name: str = "fake"
    beta_headers: list = field(default_factory=list)

    def tool_definitions(self):
        return [{"name": "noop", "description": "", "input_schema": {"type": "object"}}]

    def execute_tool_uses(self, tool_uses):
        # Echo back is_error=False for every tool_use.
        return [{"type": "tool_result", "tool_use_id": tu.id,
                 "content": [{"type": "text", "text": "{}"}],
                 "is_error": False} for tu in tool_uses]


class PassThroughVerifier:
    """Verifier the test rigs to return a specific result."""
    def __init__(self, passed: bool, details: dict | None = None):
        self._passed = passed
        self._details = details or {}

    def verify(self, transcript):
        return (self._passed, self._details)


class NoopLauncher:
    def app_class(self):
        return "test"
    def launch(self, _root):
        return 0


def make_scenario(passed: bool, budget: float = 0.50):
    return Scenario(
        code="X",
        title="t",
        model="claude-opus-4-7",
        prompt="do the thing",
        launcher=NoopLauncher(),
        verifier=PassThroughVerifier(passed),
        budget_usd=budget,
        step_cap=10,
    )


# ── Tests ──────────────────────────────────────────────────────────────

def test_loop_classifies_pass_when_verifier_passes(tmp_path: Path):
    client = FakeAnthropicClient([
        fake_resp("end_turn", [text_block("done!")]),  # No tool_use → loop exits → verify
    ])
    log = RunLogger(tmp_path / "x.jsonl", scenario="X", arm="fake", trial=1)
    result = run_loop(
        scenario=make_scenario(passed=True),
        arm=FakeArm(),
        client=client,
        logger=log,
        repo_root=tmp_path,
    )
    assert isinstance(result, LoopResult)
    assert result.classification == "pass"


def test_loop_classifies_fail_when_verifier_fails(tmp_path: Path):
    client = FakeAnthropicClient([
        fake_resp("end_turn", [text_block("giving up")]),
    ])
    log = RunLogger(tmp_path / "x.jsonl", scenario="X", arm="fake", trial=1)
    result = run_loop(
        scenario=make_scenario(passed=False),
        arm=FakeArm(),
        client=client,
        logger=log,
        repo_root=tmp_path,
    )
    # Loop returns the raw verify result; the failure-mode classification
    # (fail-tool-use-gap / fail-tool-design-gap / fail-essential) is
    # determined by the diagnosis pass (§4.11), not by the loop. The
    # loop emits "fail" and lets the diagnosis sub-classify.
    assert result.classification == "fail"


def test_loop_hits_step_cap(tmp_path: Path):
    # Always return a tool_use → loop keeps going → hits step_cap.
    responses = [
        fake_resp("tool_use", [tool_use_block("noop", {})])
        for _ in range(15)
    ]
    client = FakeAnthropicClient(responses)
    log = RunLogger(tmp_path / "x.jsonl", scenario="X", arm="fake", trial=1)
    result = run_loop(
        scenario=make_scenario(passed=False),
        arm=FakeArm(),
        client=client,
        logger=log,
        repo_root=tmp_path,
    )
    assert result.classification == "timeout-steps"


def test_loop_hits_budget_cap(tmp_path: Path):
    # Tiny budget; one step's usage already exceeds it.
    big_usage = {"input_tokens": 1_000_000, "output_tokens": 1_000_000,
                 "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    responses = [
        fake_resp("tool_use", [tool_use_block("noop", {})], usage=big_usage),
        fake_resp("tool_use", [tool_use_block("noop", {})], usage=big_usage),
    ]
    client = FakeAnthropicClient(responses)
    log = RunLogger(tmp_path / "x.jsonl", scenario="X", arm="fake", trial=1)
    result = run_loop(
        scenario=make_scenario(passed=False, budget=0.001),
        arm=FakeArm(),
        client=client,
        logger=log,
        repo_root=tmp_path,
    )
    assert result.classification == "timeout-budget"


def test_loop_emits_step_event_per_iteration(tmp_path: Path):
    import json
    client = FakeAnthropicClient([
        fake_resp("tool_use", [tool_use_block("noop", {})]),
        fake_resp("end_turn", [text_block("done")]),
    ])
    log = RunLogger(tmp_path / "x.jsonl", scenario="X", arm="fake", trial=1)
    run_loop(
        scenario=make_scenario(passed=True),
        arm=FakeArm(),
        client=client,
        logger=log,
        repo_root=tmp_path,
    )
    log.close()
    lines = [json.loads(l) for l in (tmp_path / "x.jsonl").read_text().splitlines()]
    events = [r["event"] for r in lines]
    assert "run_start" in events
    assert events.count("step") == 2
    assert "verify" in events
    assert "run_end" in events
```

- [ ] **Step 2: Run the test — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_loop.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/loop.py`**

```python
"""Shared agentic loop. Arm-agnostic.

Per spec §4.4. Non-streaming Anthropic SDK. Caps:
- BUDGET_CAP_USD per trial (from scenario.budget_usd)
- STEP_RUNAWAY_CAP per trial (from scenario.step_cap, default 50)
"""
from __future__ import annotations
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol
from harness.logger import RunLogger
from harness.pricing import cost_of_usage
from harness.scenario import Scenario


# ── Arm protocol ───────────────────────────────────────────────────────

class Arm(Protocol):
    name: str
    beta_headers: list[str]

    def tool_definitions(self) -> list[dict]:
        """Anthropic-format tool definitions. For arm B, this is the
        single computer_20251124 declaration. For arm A, the 12
        translated kde-mcp tools."""
        ...

    def execute_tool_uses(self, tool_uses: list) -> list[dict]:
        """Translate each tool_use block into a tool_result block.

        Returns one dict per input, all of which become user-message
        content blocks for the next iteration. Each result is:
            {"type": "tool_result", "tool_use_id": ...,
             "content": [...], "is_error": bool}
        """
        ...


# ── Result ─────────────────────────────────────────────────────────────

@dataclass
class LoopResult:
    classification: str  # "pass" | "fail" | "timeout-budget" | "timeout-steps"
    verify_details: dict[str, Any] = field(default_factory=dict)
    total_usd: float = 0.0
    total_steps: int = 0
    total_wallclock_ms: int = 0
    final_response_text: str = ""


# ── Loop ───────────────────────────────────────────────────────────────

def run_loop(
    *,
    scenario: Scenario,
    arm: Arm,
    client: Any,             # anthropic.Anthropic-like
    logger: RunLogger,
    repo_root: Path,
) -> LoopResult:
    """Drive a single trial. Caller owns setup/teardown — this function
    does not call scenario.setup or scenario.teardown."""

    logger.event("run_start",
                 scenario=scenario.code, arm=arm.name,
                 model=scenario.model)

    messages: list[dict] = [{"role": "user", "content": scenario.prompt}]
    transcript: list[str] = []
    spend_usd = 0.0
    start_wall = time.monotonic()

    for n in range(scenario.step_cap):
        t_req = time.time()
        resp = client.beta.messages.create(
            model=scenario.model,
            max_tokens=4096,
            tools=arm.tool_definitions(),
            betas=arm.beta_headers,
            messages=messages,
            stream=False,
        )
        t_resp = time.time()

        usage = _usage_to_dict(resp.usage)
        step_cost = cost_of_usage(usage, model=scenario.model)
        spend_usd += step_cost

        tool_uses = [b for b in resp.content if getattr(b, "type", None) == "tool_use"]
        claude_text = " ".join(
            getattr(b, "text", "") for b in resp.content
            if getattr(b, "type", None) == "text"
        )
        if claude_text:
            transcript.append(claude_text)

        logger.event(
            "step",
            n=n,
            t_request_sent=t_req,
            t_response_received=t_resp,
            usage=usage,
            tool_uses=[{"name": tu.name, "input": tu.input,
                        "tool_use_id": tu.id} for tu in tool_uses],
            claude_text=claude_text,
            stop_reason=resp.stop_reason,
        )

        logger.event("budget_check",
                     cumulative_usd=round(spend_usd, 6),
                     remaining_usd=round(scenario.budget_usd - spend_usd, 6))

        # Budget guardrail
        if spend_usd > scenario.budget_usd:
            logger.event("run_end",
                         classification="timeout-budget",
                         total_usd=spend_usd,
                         total_steps=n + 1,
                         total_wallclock_ms=int((time.monotonic() - start_wall) * 1000))
            return LoopResult(classification="timeout-budget",
                              total_usd=spend_usd, total_steps=n + 1,
                              total_wallclock_ms=int((time.monotonic() - start_wall) * 1000),
                              final_response_text=claude_text)

        # No tool_uses → Claude is done (either passed or gave up)
        if not tool_uses:
            passed, details = scenario.verify(transcript)
            logger.event("verify",
                         result="pass" if passed else "fail",
                         details=details)
            classification = "pass" if passed else "fail"
            logger.event("run_end",
                         classification=classification,
                         total_usd=spend_usd,
                         total_steps=n + 1,
                         total_wallclock_ms=int((time.monotonic() - start_wall) * 1000))
            return LoopResult(classification=classification,
                              verify_details=details,
                              total_usd=spend_usd,
                              total_steps=n + 1,
                              total_wallclock_ms=int((time.monotonic() - start_wall) * 1000),
                              final_response_text=claude_text)

        # Execute tools and feed results back
        tool_results = arm.execute_tool_uses(tool_uses)
        messages.append({"role": "assistant", "content": _content_to_dicts(resp.content)})
        messages.append({"role": "user", "content": tool_results})

    # Fell off the for-loop without returning → step cap hit
    logger.event("run_end",
                 classification="timeout-steps",
                 total_usd=spend_usd,
                 total_steps=scenario.step_cap,
                 total_wallclock_ms=int((time.monotonic() - start_wall) * 1000))
    return LoopResult(classification="timeout-steps",
                      total_usd=spend_usd,
                      total_steps=scenario.step_cap,
                      total_wallclock_ms=int((time.monotonic() - start_wall) * 1000),
                      final_response_text=transcript[-1] if transcript else "")


def _usage_to_dict(usage: Any) -> dict:
    """Convert SDK usage object to a plain dict."""
    return {
        "input_tokens": getattr(usage, "input_tokens", 0),
        "output_tokens": getattr(usage, "output_tokens", 0),
        "cache_creation_input_tokens": getattr(usage, "cache_creation_input_tokens", 0),
        "cache_read_input_tokens": getattr(usage, "cache_read_input_tokens", 0),
    }


def _content_to_dicts(content: list) -> list[dict]:
    """Convert SDK content blocks back to API-shape dicts for next turn."""
    out = []
    for b in content:
        kind = getattr(b, "type", None)
        if kind == "text":
            out.append({"type": "text", "text": b.text})
        elif kind == "tool_use":
            out.append({"type": "tool_use", "id": b.id, "name": b.name, "input": b.input})
        # Other kinds (thinking, etc.) — leave out; we're non-streaming
        # and don't use thinking.
    return out
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_loop.py -v`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add harness/src/harness/loop.py harness/tests/test_loop.py
git commit -m "feat(harness): shared agentic loop (arm-agnostic)

Non-streaming Anthropic SDK call per iteration. Caps spend
(BUDGET_CAP_USD per trial) and steps (default 50). Emits run_start /
step / budget_check / verify / run_end events. Returns LoopResult
with classification = pass | fail | timeout-budget | timeout-steps;
failure-mode sub-classification (tool-use-gap / tool-design-gap /
essential) is deferred to the diagnosis pass per §4.11."
```

---

### Task 11: Arm A — kde-mcp surface

Translates kde-mcp's `tools/list` schemas to Anthropic tool format; routes tool_uses back via `tools/call`.

**Files:**
- Create: `harness/src/harness/arms/__init__.py`
- Create: `harness/src/harness/arms/kde_mcp_arm.py`
- Create: `harness/tests/test_kde_mcp_arm.py`

- [ ] **Step 1: Create `harness/src/harness/arms/__init__.py`** (empty)

- [ ] **Step 2: Write the failing test**

`harness/tests/test_kde_mcp_arm.py`:

```python
"""Tests for arm A — kde-mcp tool surface."""
from unittest.mock import MagicMock
from harness.arms.kde_mcp_arm import KdeMcpArm, translate_tool_definitions


def test_translate_tool_definitions_drops_mcp_only_fields():
    mcp_tools = [
        {"name": "observe",
         "description": "Snapshot the AT-SPI tree.",
         "inputSchema": {"type": "object", "properties": {"window_uuid": {"type": "string"}}}},
    ]
    anth = translate_tool_definitions(mcp_tools)
    assert len(anth) == 1
    assert anth[0]["name"] == "observe"
    assert anth[0]["description"] == "Snapshot the AT-SPI tree."
    # Anthropic field name is input_schema, not inputSchema
    assert "input_schema" in anth[0]
    assert "inputSchema" not in anth[0]


def test_translate_tool_definitions_attaches_cache_control_to_last_tool():
    """Spec §5: arm A enables cache_control: ephemeral on the tools
    block so steps 2+ pay cache-read rates for tool definitions."""
    mcp_tools = [
        {"name": "a", "description": "x", "inputSchema": {"type": "object"}},
        {"name": "b", "description": "y", "inputSchema": {"type": "object"}},
    ]
    anth = translate_tool_definitions(mcp_tools)
    # Per Anthropic's docs, cache_control on the LAST tool definition
    # caches everything up to (and including) that tool definition.
    assert anth[-1].get("cache_control") == {"type": "ephemeral"}


def test_arm_executes_tool_uses_via_mcp_client():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {
        "content": [{"type": "text", "text": '{"windows": []}'}],
        "isError": False,
    }
    mcp_tools = [
        {"name": "window_query", "description": "List windows.",
         "inputSchema": {"type": "object"}},
    ]
    arm = KdeMcpArm(mock_client, mcp_tools)

    tu = type("TU", (), {"id": "tool_use_1", "name": "window_query", "input": {}})()
    results = arm.execute_tool_uses([tu])
    assert len(results) == 1
    assert results[0]["tool_use_id"] == "tool_use_1"
    assert results[0]["is_error"] is False
    mock_client.call_tool.assert_called_once_with("window_query", {})


def test_arm_propagates_mcp_error_as_is_error():
    from harness.mcp_client import MCPError
    mock_client = MagicMock()
    mock_client.call_tool.side_effect = MCPError(-32602, "Invalid params")
    mcp_tools = [{"name": "x", "description": "", "inputSchema": {"type": "object"}}]
    arm = KdeMcpArm(mock_client, mcp_tools)
    tu = type("TU", (), {"id": "t1", "name": "x", "input": {}})()
    results = arm.execute_tool_uses([tu])
    assert results[0]["is_error"] is True
    assert "Invalid params" in results[0]["content"][0]["text"]


def test_arm_beta_headers_is_empty():
    arm = KdeMcpArm(MagicMock(), [])
    assert arm.beta_headers == []
```

- [ ] **Step 3: Run the test — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_kde_mcp_arm.py -v`
Expected: ImportError.

- [ ] **Step 4: Implement `harness/src/harness/arms/kde_mcp_arm.py`**

```python
"""Arm A — declares kde-mcp's 12 wire tools to Claude and routes
tool_use blocks back to the daemon over MCP HTTP.

Schema translation:
- MCP `tools/list` returns `{name, description, inputSchema}`.
- Anthropic expects `{name, description, input_schema}` (snake_case).
- Drop any other MCP-only fields.

Caching:
- The last tool definition carries cache_control: ephemeral so steps
  2+ pay cache-read rates (~10% of input). Reported in
  response.usage.cache_read_input_tokens. Per spec §5.
"""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any
from harness.mcp_client import MCPClient, MCPError


def translate_tool_definitions(mcp_tools: list[dict]) -> list[dict]:
    """Convert MCP tools/list output to Anthropic tool format.

    Attaches cache_control: ephemeral to the LAST tool in the list so
    Anthropic caches everything up to and including that definition.
    """
    out: list[dict] = []
    for t in mcp_tools:
        out.append({
            "name": t["name"],
            "description": t.get("description", ""),
            "input_schema": t.get("inputSchema", {"type": "object"}),
        })
    if out:
        out[-1]["cache_control"] = {"type": "ephemeral"}
    return out


@dataclass
class KdeMcpArm:
    client: MCPClient
    mcp_tools: list[dict]
    name: str = "kde-mcp"
    beta_headers: list[str] = field(default_factory=list)

    def tool_definitions(self) -> list[dict]:
        return translate_tool_definitions(self.mcp_tools)

    def execute_tool_uses(self, tool_uses: list) -> list[dict]:
        results: list[dict] = []
        for tu in tool_uses:
            try:
                mcp_result = self.client.call_tool(tu.name, tu.input)
                # mcp_result is {"content": [...], "isError": bool}
                content = mcp_result.get("content", [])
                is_error = mcp_result.get("isError", False)
                results.append({
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": content,
                    "is_error": is_error,
                })
            except MCPError as e:
                results.append({
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": [{"type": "text",
                                 "text": f"MCP error {e.code}: {e}"}],
                    "is_error": True,
                })
        return results
```

- [ ] **Step 5: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_kde_mcp_arm.py -v`
Expected: 5 tests pass.

- [ ] **Step 6: Commit**

```bash
git add harness/src/harness/arms/__init__.py harness/src/harness/arms/kde_mcp_arm.py harness/tests/test_kde_mcp_arm.py
git commit -m "feat(harness): arm A — kde-mcp tool surface

Translates kde-mcp's tools/list to Anthropic tool format (snake_case
input_schema); routes tool_use blocks back via tools/call. Last tool
carries cache_control: ephemeral so tool definitions hit the prompt
cache from step 2 onwards (~10% input rate). Per spec §4.5 + §5."
```

---

### Task 12: Arm B — vision/computer-use surface

Declares the single `computer_20251124` tool to Claude; on each tool_use, translates the action to kde-mcp screenshot/mouse/keyboard calls via the bridge table from spec §4.6.

**Files:**
- Create: `harness/src/harness/arms/vision_arm.py`
- Create: `harness/tests/test_vision_arm.py`

- [ ] **Step 1: Write the failing test**

`harness/tests/test_vision_arm.py`:

```python
"""Tests for arm B — Anthropic computer_20251124 bridged to kde-mcp."""
from unittest.mock import MagicMock
from harness.arms.vision_arm import VisionArm, BRIDGE_TABLE


def test_tool_definitions_declares_computer_only():
    arm = VisionArm(MagicMock(), display_width=1920, display_height=1080)
    defs = arm.tool_definitions()
    assert len(defs) == 1
    assert defs[0]["type"] == "computer_20251124"
    assert defs[0]["name"] == "computer"
    assert defs[0]["display_width_px"] == 1920
    assert defs[0]["display_height_px"] == 1080


def test_beta_headers_includes_computer_use():
    arm = VisionArm(MagicMock())
    assert "computer-use-2025-11-24" in arm.beta_headers


def test_bridge_table_covers_all_spec_actions():
    """Spec §4.6 lists 16 actions; every one must map."""
    expected = {
        "screenshot", "left_click", "right_click", "middle_click",
        "double_click", "triple_click", "mouse_move", "left_click_drag",
        "left_mouse_down", "left_mouse_up", "scroll", "type", "key",
        "hold_key", "wait", "zoom",
    }
    assert expected == set(BRIDGE_TABLE.keys())


def test_left_click_translates_to_mouse_call():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)

    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "left_click",
                                   "coordinate": [100, 200]}})()
    arm.execute_tool_uses([tu])
    mock_client.call_tool.assert_called_once()
    name, args = mock_client.call_tool.call_args[0]
    assert name == "mouse"
    assert args["action"] == "click"
    assert args["button"] == "left"
    assert args["coordinate"] == [100, 200]


def test_left_click_drag_translates_with_start_and_end():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)

    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "left_click_drag",
                                   "start_coordinate": [10, 20],
                                   "coordinate": [100, 200]}})()
    arm.execute_tool_uses([tu])
    name, args = mock_client.call_tool.call_args[0]
    assert name == "mouse"
    assert args["action"] == "drag"
    assert args["start_coordinate"] == [10, 20]
    assert args["coordinate"] == [100, 200]


def test_double_and_triple_click_use_count():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)

    for action, expected_count in [("double_click", 2), ("triple_click", 3)]:
        mock_client.reset_mock()
        tu = type("TU", (), {"id": "t1", "name": "computer",
                             "input": {"action": action,
                                       "coordinate": [10, 20]}})()
        arm.execute_tool_uses([tu])
        _name, args = mock_client.call_tool.call_args[0]
        assert args["count"] == expected_count


def test_type_translates_to_keyboard_type():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)
    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "type", "text": "hello"}})()
    arm.execute_tool_uses([tu])
    name, args = mock_client.call_tool.call_args[0]
    assert name == "keyboard_type"
    assert args["text"] == "hello"


def test_key_translates_to_keyboard_key():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)
    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "key", "text": "ctrl+a"}})()
    arm.execute_tool_uses([tu])
    name, args = mock_client.call_tool.call_args[0]
    assert name == "keyboard_key"
    assert args["keys"] == "ctrl+a"


def test_hold_key_translates_to_keyboard_key_with_hold_ms():
    mock_client = MagicMock()
    mock_client.call_tool.return_value = {"content": [], "isError": False}
    arm = VisionArm(mock_client)
    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "hold_key", "text": "shift",
                                   "duration": 2}})()
    arm.execute_tool_uses([tu])
    name, args = mock_client.call_tool.call_args[0]
    assert name == "keyboard_key"
    assert args["keys"] == "shift"
    assert args["hold_ms"] == 2000


def test_wait_is_host_side_no_mcp_call():
    """`wait` doesn't hit kde-mcp — host-side time.sleep."""
    import time
    mock_client = MagicMock()
    arm = VisionArm(mock_client)
    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "wait", "duration": 0.05}})()
    t0 = time.monotonic()
    results = arm.execute_tool_uses([tu])
    elapsed = time.monotonic() - t0
    assert elapsed >= 0.05
    mock_client.call_tool.assert_not_called()
    assert results[0]["is_error"] is False


def test_unknown_action_emits_bridge_gap_is_error():
    mock_client = MagicMock()
    arm = VisionArm(mock_client)
    tu = type("TU", (), {"id": "t1", "name": "computer",
                         "input": {"action": "some_future_action"}})()
    results = arm.execute_tool_uses([tu])
    assert results[0]["is_error"] is True
    text = results[0]["content"][0]["text"]
    assert "bridge-gap" in text
    assert "some_future_action" in text
```

- [ ] **Step 2: Run the test — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_vision_arm.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/arms/vision_arm.py`**

```python
"""Arm B — Anthropic's computer_20251124 bridged to kde-mcp.

Declares exactly one tool to Claude (the schema-less `computer` tool).
On each tool_use, translates the action to a kde-mcp tools/call via
the BRIDGE_TABLE. Actions not in the table return is_error=True with
a 'bridge-gap' message — that itself becomes a finding in the
diagnosis (it means a future Anthropic action variant escaped the
table we maintain).

Bridge mapping verified against src/tools/mouse.rs (actions: click,
move, drag, scroll, down, up, hold) and src/tools/keyboard.rs
(keyboard_type, keyboard_key with optional hold_ms). All 16 current
Anthropic actions map. Per spec §4.6.
"""
from __future__ import annotations
import time
from dataclasses import dataclass, field
from typing import Any, Callable
from harness.mcp_client import MCPClient, MCPError


# Each bridge entry is a callable: (input_dict, client) → tool_result dict
def _wrap_mcp(name: str, builder: Callable[[dict], dict]):
    """Wrap an arg-builder so it ends up calling client.call_tool(name, args)."""
    def runner(inp: dict, client: MCPClient) -> dict:
        args = builder(inp)
        result = client.call_tool(name, args)
        return {
            "content": result.get("content", []),
            "is_error": result.get("isError", False),
        }
    return runner


def _host_sleep(inp: dict, _client) -> dict:
    duration = float(inp.get("duration", 0.5))
    time.sleep(duration)
    return {"content": [{"type": "text",
                         "text": f"waited {duration}s"}],
            "is_error": False}


def _host_zoom(inp: dict, client: MCPClient) -> dict:
    """Anthropic's zoom = re-screenshot of a sub-region. We approximate
    by re-screenshotting the full screen (kde-mcp's screenshot tool
    supports max_long_edge; cropping happens host-side if we ever need
    it). Simplest viable: just take another screenshot."""
    result = client.call_tool("screenshot", {})
    return {
        "content": result.get("content", []),
        "is_error": result.get("isError", False),
    }


BRIDGE_TABLE: dict[str, Callable[[dict, Any], dict]] = {
    "screenshot": _wrap_mcp("screenshot", lambda inp: {}),
    "left_click": _wrap_mcp("mouse", lambda inp: {
        "action": "click", "button": "left",
        "coordinate": inp.get("coordinate"),
    }),
    "right_click": _wrap_mcp("mouse", lambda inp: {
        "action": "click", "button": "right",
        "coordinate": inp.get("coordinate"),
    }),
    "middle_click": _wrap_mcp("mouse", lambda inp: {
        "action": "click", "button": "middle",
        "coordinate": inp.get("coordinate"),
    }),
    "double_click": _wrap_mcp("mouse", lambda inp: {
        "action": "click", "button": "left", "count": 2,
        "coordinate": inp.get("coordinate"),
    }),
    "triple_click": _wrap_mcp("mouse", lambda inp: {
        "action": "click", "button": "left", "count": 3,
        "coordinate": inp.get("coordinate"),
    }),
    "mouse_move": _wrap_mcp("mouse", lambda inp: {
        "action": "move", "coordinate": inp.get("coordinate"),
    }),
    "left_click_drag": _wrap_mcp("mouse", lambda inp: {
        "action": "drag",
        "start_coordinate": inp.get("start_coordinate"),
        "coordinate": inp.get("coordinate"),
        "button": "left",
    }),
    "left_mouse_down": _wrap_mcp("mouse", lambda inp: {
        "action": "down", "button": "left",
        "coordinate": inp.get("coordinate"),
    }),
    "left_mouse_up": _wrap_mcp("mouse", lambda inp: {
        "action": "up", "button": "left",
    }),
    "scroll": _wrap_mcp("mouse", lambda inp: {
        "action": "scroll",
        "coordinate": inp.get("coordinate"),
        "scroll_direction": inp.get("scroll_direction", "down"),
        "scroll_amount": inp.get("scroll_amount", 3),
    }),
    "type": _wrap_mcp("keyboard_type", lambda inp: {"text": inp.get("text", "")}),
    "key": _wrap_mcp("keyboard_key", lambda inp: {"keys": inp.get("text", "")}),
    "hold_key": _wrap_mcp("keyboard_key", lambda inp: {
        "keys": inp.get("text", ""),
        # Anthropic gives `duration` in seconds; kde-mcp wants ms.
        "hold_ms": int(float(inp.get("duration", 0)) * 1000),
    }),
    "wait": _host_sleep,
    "zoom": _host_zoom,
}


@dataclass
class VisionArm:
    client: MCPClient
    display_width: int = 1920
    display_height: int = 1080
    name: str = "vision"
    beta_headers: list[str] = field(
        default_factory=lambda: ["computer-use-2025-11-24"]
    )

    def tool_definitions(self) -> list[dict]:
        return [{
            "type": "computer_20251124",
            "name": "computer",
            "display_width_px": self.display_width,
            "display_height_px": self.display_height,
        }]

    def execute_tool_uses(self, tool_uses: list) -> list[dict]:
        results: list[dict] = []
        for tu in tool_uses:
            action = tu.input.get("action") if isinstance(tu.input, dict) else None
            handler = BRIDGE_TABLE.get(action) if action else None
            if handler is None:
                results.append({
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": [{"type": "text",
                                 "text": f"bridge-gap: action '{action}' not in bridge table"}],
                    "is_error": True,
                })
                continue
            try:
                result = handler(tu.input, self.client)
                results.append({
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": result["content"],
                    "is_error": result["is_error"],
                })
            except MCPError as e:
                results.append({
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": [{"type": "text",
                                 "text": f"MCP error {e.code}: {e}"}],
                    "is_error": True,
                })
        return results
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_vision_arm.py -v`
Expected: 10 tests pass.

- [ ] **Step 5: Commit**

```bash
git add harness/src/harness/arms/vision_arm.py harness/tests/test_vision_arm.py
git commit -m "feat(harness): arm B — vision/computer-use bridged to kde-mcp

Declares the schema-less computer_20251124 tool with the
computer-use-2025-11-24 beta header. BRIDGE_TABLE maps all 16
Anthropic computer-use actions to kde-mcp tools (verified against
src/tools/mouse.rs and src/tools/keyboard.rs). wait + zoom are host-
side. Unknown actions return is_error with a bridge-gap message,
which the loop logs as an annotation per §4.6. Per spec §4.6."
```

---

## Phase F — Fixtures + scenarios

Four hand-built fixtures, one per (E, Q, D, R). Each task ships the fixture + the scenario YAML in a single commit.

### Task 13: Fixture E — `probe-canvas.html` (canvas-digit buttons)

**Files:**
- Create: `harness/fixtures/probe-canvas.html`
- Create: `harness/scenarios/E.yaml`

- [ ] **Step 1: Create `harness/fixtures/probe-canvas.html`**

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>kde-mcp probe E — canvas digits</title>
  <style>
    body { margin: 40px; font-family: sans-serif; background: #fafafa; }
    h1 { color: #333; }
    .row { display: flex; gap: 16px; margin-top: 24px; }
    canvas { border: 1px solid #888; cursor: pointer; background: #fff; }
    .log { margin-top: 24px; padding: 12px; background: #eef;
           font-family: monospace; min-height: 1em; }
  </style>
</head>
<body>
  <h1>Click a button</h1>
  <p>Five buttons below. Each shows a digit drawn into a canvas. There
     is no DOM text for the digit — only pixels.</p>
  <div class="row" id="row"></div>
  <div class="log" id="log">(no clicks yet)</div>

  <script>
    // Five canvas buttons rendered as <canvas> elements showing the
    // digits 1..5. AT-SPI sees them as five indistinguishable
    // <canvas> nodes — no accessible name for the digit. Vision reads
    // the pixels.
    const row = document.getElementById("row");
    const log = document.getElementById("log");
    for (let i = 1; i <= 5; i++) {
      const c = document.createElement("canvas");
      c.width = 80;
      c.height = 80;
      c.dataset.index = String(i);
      const ctx = c.getContext("2d");
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, 80, 80);
      ctx.fillStyle = "#222";
      ctx.font = "56px sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(i), 40, 44);
      c.addEventListener("click", () => {
        log.textContent = `clicked: ${i}`;
        // Write the clicked index to /tmp/probe-e.txt via fetch to a
        // local helper, OR — simpler and Brave-friendly — use the
        // browser's clipboard API + a polling helper. Simplest of
        // all: have the harness watch the page title.
        document.title = `clicked-${i}`;
      });
      row.appendChild(c);
    }
  </script>
</body>
</html>
```

**Verifier-write decision:** the HTML can't write directly to `/tmp/` from the browser sandbox. Use document.title polling — the harness's scenario.setup() spawns a polling thread that watches the page title via kde-mcp's `inspect` tool, and writes the clicked index to `/tmp/probe-e.txt` on the host. Simpler alternative (chosen here): the verifier reads the **window title** via `wmctrl` / `kwin window_query` instead of `/tmp/probe-e.txt`.

Update the scenario YAML accordingly (see step 2). And update the verifier in `scenario.py` to support `window_title_contains` — add to `_make_verifier`:

```python
    if kind == "window_title_contains":
        return WindowTitleContainsVerifier(
            app_class=spec["app_class"],
            expect=spec["expect"],
        )
```

And the verifier itself in `scenario.py`:

```python
@dataclass
class WindowTitleContainsVerifier:
    app_class: str
    expect: str

    def verify(self, transcript: list[str]) -> tuple[bool, dict[str, Any]]:
        # Hit kde-mcp's window_query to read the current title. We do
        # this via the same MCPClient instance the loop used; the
        # scenario carries no client of its own, so we use subprocess
        # to `curl` /mcp once. Simpler: read via xdotool fallback if
        # available, else fail with a clear message.
        import subprocess
        try:
            out = subprocess.run(
                ["wmctrl", "-l", "-x"], capture_output=True, text=True, timeout=2,
            )
            for line in out.stdout.splitlines():
                if self.app_class in line and self.expect in line:
                    return True, {"matched_line": line.strip()}
        except (FileNotFoundError, subprocess.TimeoutExpired):
            return False, {"reason": "wmctrl-unavailable"}
        return False, {"reason": "no-matching-title", "expect": self.expect}
```

This makes `scenario.py` self-contained for the verify step — it doesn't depend on the MCP client at verify time.

- [ ] **Step 2: Create `harness/scenarios/E.yaml`**

```yaml
code: E
title: Canvas-digit buttons (Brave)
model: claude-opus-4-7
prompt: |
  This window has five buttons drawn as HTML <canvas> elements. Each
  button shows one digit, 1 through 5. Click the button showing the
  number 3.
fixture:
  type: html
  path: harness/fixtures/probe-canvas.html
  app_class: kde-mcp-probe-canvas
verify:
  type: window_title_contains
  app_class: kde-mcp-probe-canvas
  expect: "clicked-3"
budget_usd: 0.50
step_cap: 50
```

- [ ] **Step 3: Add the `WindowTitleContainsVerifier` to `scenario.py` (per step 1's note)**

Update `harness/src/harness/scenario.py` with the new verifier class and the `_make_verifier` case. Add a test in `harness/tests/test_scenario.py`:

```python
def test_load_E_scenario():
    s = load_scenario(Path("harness/scenarios/E.yaml"))
    assert s.code == "E"
    assert s.launcher.app_class() == "kde-mcp-probe-canvas"
```

Run: `cd harness && .venv/bin/pytest tests/test_scenario.py -v`
Expected: pass.

- [ ] **Step 4: Manual smoke**

Run:
```bash
brave --class=kde-mcp-probe-canvas "file://$(pwd)/harness/fixtures/probe-canvas.html"
```

Verify visually: 5 white squares with digits 1–5; clicking digit 3 changes window title to "clicked-3"; `wmctrl -l -x | grep canvas` shows the title.

- [ ] **Step 5: Commit**

```bash
git add harness/fixtures/probe-canvas.html harness/scenarios/E.yaml harness/src/harness/scenario.py harness/tests/test_scenario.py
git commit -m "feat(harness): fixture E — canvas-digit buttons + scenario

Brave page with 5 <canvas> elements showing digits 1..5; no DOM
text for the digit. Clicking sets the window title to clicked-<N>;
the WindowTitleContainsVerifier reads the title via wmctrl. AT-SPI
exposes 5 indistinguishable canvas nodes; vision must read pixels.
Per spec §4.2 scenario E."
```

---

### Task 14: Fixture Q — `probe-qt-noname.py` (Qt no-accessibleName)

PySide6 window with 3 push-buttons labeled "Alpha"/"Beta"/"Gamma" visually, but no `setAccessibleName()` call. Clicking writes the index to `/tmp/probe-q.txt`.

**Files:**
- Create: `harness/fixtures/probe-qt-noname.py`
- Create: `harness/scenarios/Q.yaml`

- [ ] **Step 1: Create `harness/fixtures/probe-qt-noname.py`**

```python
#!/usr/bin/env python
"""Fixture Q — Qt window with display text but no accessibleName.

AT-SPI sees three buttons with no accessible name. Visually they're
labeled Alpha / Beta / Gamma. Click the Beta button; the click handler
writes "1" to /tmp/probe-q.txt.
"""
import sys
from PySide6.QtWidgets import (QApplication, QWidget, QPushButton, QHBoxLayout,
                                QLabel, QVBoxLayout)


LABELS = ["Alpha", "Beta", "Gamma"]


class Probe(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("kde-mcp probe Q")
        self.resize(420, 160)

        outer = QVBoxLayout(self)
        outer.addWidget(QLabel("Click the Beta button."))
        row = QHBoxLayout()
        for i, text in enumerate(LABELS):
            btn = QPushButton(text)
            # DELIBERATELY no setAccessibleName(). The visible text on
            # the QPushButton is what we want vision to read; we want
            # AT-SPI to see "<no name>" or to fall back to the visible
            # text in a way the agent can or can't rely on.
            btn.clicked.connect(lambda _checked=False, idx=i: self.on_click(idx))
            row.addWidget(btn)
        outer.addLayout(row)

    @staticmethod
    def on_click(index: int) -> None:
        with open("/tmp/probe-q.txt", "w") as f:
            f.write(str(index))


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("kde-mcp-probe-qt-noname")
    app.setApplicationDisplayName("kde-mcp probe Q")
    # Setting desktopFileName makes KWin report the class as we expect.
    app.setDesktopFileName("kde-mcp-probe-qt-noname")
    w = Probe()
    w.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Create `harness/scenarios/Q.yaml`**

```yaml
code: Q
title: Qt buttons with no accessibleName
model: claude-opus-4-7
prompt: |
  This window has three buttons labeled Alpha, Beta, and Gamma. The
  labels are visible on the buttons. Click the Beta button.
fixture:
  type: python
  path: harness/fixtures/probe-qt-noname.py
  app_class: kde-mcp-probe-qt-noname
verify:
  type: file_contains
  path: /tmp/probe-q.txt
  expect: "1"
budget_usd: 0.50
step_cap: 50
```

- [ ] **Step 3: Manual smoke**

```bash
python harness/fixtures/probe-qt-noname.py
# Click Beta in the window, then:
cat /tmp/probe-q.txt   # should print: 1
rm /tmp/probe-q.txt    # cleanup between runs
```

- [ ] **Step 4: Inspect AT-SPI exposure to confirm the "weakness"**

Run kde-mcp's `inspect` tool against the Beta button (via curl or via a quick harness invocation). Confirm the AT-SPI name is empty/anonymous. If KDE reports the visible text via a different AT-SPI attribute (e.g., AT-SPI 2.x sometimes exposes the rendered text), document the result in the commit message — the fixture might be a weaker probe than designed, and the diagnosis pass needs to account for that.

- [ ] **Step 5: Commit**

```bash
git add harness/fixtures/probe-qt-noname.py harness/scenarios/Q.yaml
git commit -m "feat(harness): fixture Q — Qt no-accessibleName + scenario

PySide6 window with three QPushButtons labeled Alpha/Beta/Gamma
visually; deliberately no setAccessibleName(). app_class is
kde-mcp-probe-qt-noname so the policy curation step can pin it.
Click writes the button index to /tmp/probe-q.txt. Per spec §4.2
scenario Q."
```

---

### Task 15: Fixture D — `probe-drag.py` (drag-precision)

PySide6 QGraphicsView with a 20×20 handle at (100, 100). Drag the handle 60 pixels right. Final x must end up in [155, 165].

**Files:**
- Create: `harness/fixtures/probe-drag.py`
- Create: `harness/scenarios/D.yaml`

- [ ] **Step 1: Create `harness/fixtures/probe-drag.py`**

```python
#!/usr/bin/env python
"""Fixture D — drag-precision probe.

A 20x20 handle starts at (100, 100). Drag it 60 pixels right. On
release, write the handle's final (x, y) center to /tmp/probe-d.txt
as "x,y".
"""
import sys
from PySide6.QtCore import QPointF, Qt
from PySide6.QtWidgets import (QApplication, QGraphicsScene, QGraphicsView,
                                QGraphicsRectItem)


class Handle(QGraphicsRectItem):
    def __init__(self):
        # Rect is centered on the item's origin.
        super().__init__(-10, -10, 20, 20)
        self.setBrush(Qt.darkGreen)
        self.setFlag(QGraphicsRectItem.GraphicsItemFlag.ItemIsMovable, True)
        self.setFlag(QGraphicsRectItem.GraphicsItemFlag.ItemSendsGeometryChanges, True)
        self.setPos(100, 100)

    def mouseReleaseEvent(self, event):
        super().mouseReleaseEvent(event)
        c: QPointF = self.scenePos()
        with open("/tmp/probe-d.txt", "w") as f:
            f.write(f"{c.x()},{c.y()}")


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("kde-mcp-probe-drag")
    app.setApplicationDisplayName("kde-mcp probe D")
    app.setDesktopFileName("kde-mcp-probe-drag")

    scene = QGraphicsScene(0, 0, 400, 200)
    scene.addItem(Handle())
    view = QGraphicsView(scene)
    view.setWindowTitle("kde-mcp probe D")
    view.resize(420, 220)
    view.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Create `harness/scenarios/D.yaml`**

```yaml
code: D
title: Drag the handle 60 pixels right
model: claude-opus-4-7
prompt: |
  This window shows a green square handle at coordinates (100, 100).
  Drag the handle 60 pixels to the right. The handle is 20 pixels
  wide.
fixture:
  type: python
  path: harness/fixtures/probe-drag.py
  app_class: kde-mcp-probe-drag
verify:
  type: coord_range
  path: /tmp/probe-d.txt
  axis: x
  lo: 155
  hi: 165
budget_usd: 0.50
step_cap: 50
```

- [ ] **Step 3: Manual smoke**

```bash
rm -f /tmp/probe-d.txt
python harness/fixtures/probe-drag.py
# Drag the green square right manually, release.
cat /tmp/probe-d.txt    # should print e.g. "160.0,100.0"
```

- [ ] **Step 4: Commit**

```bash
git add harness/fixtures/probe-drag.py harness/scenarios/D.yaml
git commit -m "feat(harness): fixture D — drag-precision + scenario

PySide6 QGraphicsView with a 20x20 movable handle starting at
(100, 100). On release writes the final centre coordinates to
/tmp/probe-d.txt. Verifier asserts final x ∈ [155, 165]. Per spec
§4.2 scenario D."
```

---

### Task 16: Fixture R — `probe-chart.png` (rasterised chart)

A pre-rendered bar-chart PNG with four bars labeled 12 / 73 / 45 / 31. Opens in Gwenview. AT-SPI has nothing to offer — must use OCR (`read_text`) or vision.

**Files:**
- Create: `harness/fixtures/probe-chart.gen.py` (generator; committed for reproducibility)
- Create: `harness/fixtures/probe-chart.png` (output of the generator)
- Create: `harness/scenarios/R.yaml`

- [ ] **Step 1: Create the generator `harness/fixtures/probe-chart.gen.py`**

```python
#!/usr/bin/env python
"""Generates probe-chart.png — a bar chart with four labeled bars.

Run once; commit the resulting PNG. Re-run if the chart shape ever
needs to change.
"""
from PIL import Image, ImageDraw, ImageFont


BARS = [("A", 12), ("B", 73), ("C", 45), ("D", 31)]


def main():
    width, height = 800, 500
    img = Image.new("RGB", (width, height), "white")
    draw = ImageDraw.Draw(img)

    # Try a hopefully-installed font; fall back to default.
    try:
        font = ImageFont.truetype("/usr/share/fonts/liberation/LiberationSans-Bold.ttf", 28)
        font_small = ImageFont.truetype("/usr/share/fonts/liberation/LiberationSans-Regular.ttf", 20)
    except (OSError, IOError):
        font = ImageFont.load_default()
        font_small = font

    # Axes
    draw.line([(80, 50), (80, 450)], fill="black", width=2)
    draw.line([(80, 450), (760, 450)], fill="black", width=2)
    draw.text((300, 10), "Bar chart", fill="black", font=font)

    # Bars
    bar_w = 100
    gap = 60
    x = 140
    max_v = max(v for _, v in BARS)
    for label, v in BARS:
        h = int((v / max_v) * 380)
        y = 450 - h
        draw.rectangle([x, y, x + bar_w, 450], outline="black", fill="lightsteelblue", width=2)
        # Numeric label above the bar
        draw.text((x + bar_w / 2 - 18, y - 30), str(v), fill="black", font=font)
        # Category label below the axis
        draw.text((x + bar_w / 2 - 8, 460), label, fill="black", font=font_small)
        x += bar_w + gap

    img.save("harness/fixtures/probe-chart.png")
    print(f"wrote harness/fixtures/probe-chart.png ({width}x{height})")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Generate the PNG**

```bash
pip install --user Pillow  # if not already in dev deps
cd ~/path/to/kde-mcp
python harness/fixtures/probe-chart.gen.py
```

Expected: `harness/fixtures/probe-chart.png` exists. Open it visually to confirm four bars labeled 12 / 73 / 45 / 31 with the tallest being 73.

(`Pillow` is a one-off dev-time tool, not a runtime dep. Don't add it to `pyproject.toml`. If the user doesn't have it, document `pip install --user Pillow` in `harness/README.md` under the section "Regenerating the chart".)

- [ ] **Step 3: Create `harness/scenarios/R.yaml`**

```yaml
code: R
title: Read a rasterised bar chart
model: claude-opus-4-7
prompt: |
  A bar chart is open in the image viewer. It has four bars. Tell me
  the value of the tallest bar.
fixture:
  type: image
  path: harness/fixtures/probe-chart.png
verify:
  type: response_contains
  expect: "73"
budget_usd: 0.50
step_cap: 50
```

- [ ] **Step 4: Manual smoke**

```bash
gwenview harness/fixtures/probe-chart.png
# Visually confirm the tallest bar reads "73".
```

- [ ] **Step 5: Commit**

```bash
git add harness/fixtures/probe-chart.gen.py harness/fixtures/probe-chart.png harness/scenarios/R.yaml
git commit -m "feat(harness): fixture R — rasterised bar chart + scenario

Pre-rendered PNG opened in Gwenview. Four bars labeled 12 / 73 / 45 /
31; tallest is 73. AT-SPI exposes nothing about the image content;
text-only must call read_text (Tesseract OCR), vision reads the
bars directly. ResponseContainsVerifier asserts '73' appears in the
final assistant message. Generator script committed for
reproducibility. Per spec §4.2 scenario R."
```

---

## Phase G — Orchestration

Two tasks: CLI entry point (subcommands wire everything together) and summary.md emitter.

### Task 17: CLI entry point

`harness` subcommands: `smoke`, `setup-policy`, `run`, `run-all`, `report`. Driven via `python -m harness.cli` or the installed `harness` console script.

**Files:**
- Create: `harness/src/harness/cli.py`
- Create: `harness/tests/test_cli.py`

- [ ] **Step 1: Write the failing test**

`harness/tests/test_cli.py`:

```python
"""Tests for the CLI argument parsing.

The actual subcommand bodies are exercised end-to-end in Task 19's
live run — testing them with mocks here would just verify the mocks."""
import pytest
from harness.cli import build_parser


def test_parser_accepts_smoke():
    p = build_parser()
    args = p.parse_args(["smoke"])
    assert args.command == "smoke"


def test_parser_accepts_setup_policy():
    p = build_parser()
    args = p.parse_args(["setup-policy"])
    assert args.command == "setup-policy"


def test_parser_accepts_run_with_required_flags():
    p = build_parser()
    args = p.parse_args(["run", "--scenario", "E", "--arm", "vision", "--trial", "1"])
    assert args.command == "run"
    assert args.scenario == "E"
    assert args.arm == "vision"
    assert args.trial == 1


def test_parser_accepts_run_all():
    p = build_parser()
    args = p.parse_args(["run-all"])
    assert args.command == "run-all"


def test_parser_run_arm_choices():
    p = build_parser()
    with pytest.raises(SystemExit):
        p.parse_args(["run", "--scenario", "E", "--arm", "bogus", "--trial", "1"])


def test_parser_report_takes_runs_dir():
    p = build_parser()
    args = p.parse_args(["report", "--runs", "harness/runs/20260519-1200"])
    assert args.command == "report"
    assert args.runs == "harness/runs/20260519-1200"
```

- [ ] **Step 2: Run the test — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_cli.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/cli.py`**

```python
"""Harness CLI.

Subcommands:
- smoke         — Phase 0 smoke test (~$0.01)
- setup-policy  — launch each fixture once, query its KWin class,
                  emit a YAML snippet to paste into policy.toml
- run           — single (scenario × arm × trial)
- run-all       — all scenarios × both arms × 2 trials each, plus
                  tiebreakers on pass/fail disagreement
- report        — re-emit summary.md from a runs/<ts>/ directory

Daemon prerequisite: kde-mcp must be running at 127.0.0.1:7575.
The CLI hits /healthz at startup and fails loudly if it's unreachable.
"""
from __future__ import annotations
import argparse
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCENARIOS_DIR = REPO_ROOT / "harness" / "scenarios"
RUNS_DIR = REPO_ROOT / "harness" / "runs"


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="harness", description="kde-mcp Study 2 harness")
    sub = p.add_subparsers(dest="command", required=True)

    sub.add_parser("smoke", help="Phase 0 smoke test (~$0.01)")
    sub.add_parser("setup-policy", help="Launch fixtures once to capture KWin classes")

    p_run = sub.add_parser("run", help="Single trial")
    p_run.add_argument("--scenario", required=True, choices=["E", "Q", "D", "R"])
    p_run.add_argument("--arm", required=True, choices=["kde-mcp", "vision"])
    p_run.add_argument("--trial", required=True, type=int)

    sub.add_parser("run-all", help="All scenarios × both arms × 2 trials")

    p_report = sub.add_parser("report", help="Re-emit summary.md")
    p_report.add_argument("--runs", required=True, help="Path to a runs/<timestamp>/ dir")

    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.command == "smoke":
        from harness.smoke import run_smoke
        result = run_smoke()
        print(f"smoke: {result.status}{(' — ' + result.detail) if result.detail else ''}")
        return 0 if result.ok else 1

    if args.command == "setup-policy":
        return _setup_policy()

    if args.command == "run":
        return _run_single(args.scenario, args.arm, args.trial)

    if args.command == "run-all":
        return _run_all()

    if args.command == "report":
        return _report(Path(args.runs))

    return 2  # unreachable; argparse already enforces required subcommand


# ── subcommand bodies ─────────────────────────────────────────────────

def _check_daemon() -> None:
    import httpx
    try:
        r = httpx.get("http://127.0.0.1:7575/healthz", timeout=2.0)
        r.raise_for_status()
        info = r.json()
        print(f"kde-mcp daemon: ok (version={info.get('version')}, "
              f"git_sha={info.get('git_sha')})")
    except Exception as e:
        print(f"kde-mcp daemon unreachable at 127.0.0.1:7575: {e}", file=sys.stderr)
        print("Start it with: systemctl --user start kde-mcp-http.service",
              file=sys.stderr)
        raise SystemExit(2)


def _setup_policy() -> int:
    """Launch each fixture once, query its KWin class via kde-mcp's
    window_query, print a YAML snippet to paste into policy.toml."""
    from harness.mcp_client import MCPClient
    from harness.scenario import load_scenario

    _check_daemon()
    client = MCPClient("http://127.0.0.1:7575/mcp")
    client.initialize()

    snippets: list[str] = []
    for code in ("E", "Q", "D", "R"):
        scenario = load_scenario(SCENARIOS_DIR / f"{code}.yaml")
        print(f"\n─── scenario {code} ───")
        scenario.setup(REPO_ROOT)
        try:
            time.sleep(2.0)  # let the window settle
            # Query KWin for windows whose class matches what we expect.
            result = client.call_tool("window_query", {})
            for c in result.get("content", []):
                if c.get("type") == "text":
                    print(f"window_query → {c['text'][:500]}")
            snippets.append(
                f"[apps.\"{scenario.launcher.app_class()}\"]\n"
                f"tier = \"full\"\n"
            )
        finally:
            scenario.teardown()

    print("\n─── policy.toml snippet ───")
    print("# Paste under ~/.config/kde-mcp/policy.toml:\n")
    for snip in snippets:
        print(snip)
    return 0


def _make_clients_and_arms():
    """Construct an MCPClient + both arms once per run-session."""
    from harness.mcp_client import MCPClient
    from harness.arms.kde_mcp_arm import KdeMcpArm
    from harness.arms.vision_arm import VisionArm

    mcp = MCPClient("http://127.0.0.1:7575/mcp")
    mcp.initialize()
    tools = mcp.list_tools()
    kde_mcp_arm = KdeMcpArm(mcp, tools)
    # Resolve display geometry: ask kde-mcp.
    geo_result = mcp.call_tool("window_query", {})
    # Fallback: assume 1920x1080 if anything goes wrong reading geometry.
    vision_arm = VisionArm(mcp)  # display defaults are 1920x1080
    return mcp, kde_mcp_arm, vision_arm


def _run_single(code: str, arm_name: str, trial: int) -> int:
    from anthropic import Anthropic
    from harness.scenario import load_scenario
    from harness.logger import RunLogger
    from harness.loop import run_loop

    _check_daemon()

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("ANTHROPIC_API_KEY not set", file=sys.stderr)
        return 2

    os.environ.setdefault("KDE_MCP_SESSION_TAG", "harness")

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = RUNS_DIR / timestamp
    run_dir.mkdir(parents=True, exist_ok=True)

    scenario = load_scenario(SCENARIOS_DIR / f"{code}.yaml")
    _, kde_mcp_arm, vision_arm = _make_clients_and_arms()
    arm = kde_mcp_arm if arm_name == "kde-mcp" else vision_arm

    log_path = run_dir / f"{code}-{arm_name}-{trial}.jsonl"
    client = Anthropic(api_key=api_key)
    with RunLogger(log_path, scenario=code, arm=arm_name, trial=trial) as log:
        scenario.setup(REPO_ROOT)
        try:
            result = run_loop(
                scenario=scenario, arm=arm, client=client,
                logger=log, repo_root=REPO_ROOT,
            )
        finally:
            scenario.teardown()
    print(f"{code}-{arm_name}-{trial}: {result.classification} "
          f"(${result.total_usd:.4f}, {result.total_steps} steps)")
    return 0


def _run_all() -> int:
    """Run 4 scenarios × 2 arms × 2 trials with tiebreakers on disagreement."""
    from anthropic import Anthropic
    from harness.scenario import load_scenario
    from harness.logger import RunLogger
    from harness.loop import run_loop
    from harness.report import emit_summary
    import json

    _check_daemon()
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("ANTHROPIC_API_KEY not set", file=sys.stderr)
        return 2
    os.environ.setdefault("KDE_MCP_SESSION_TAG", "harness")

    budget_session = float(os.environ.get("BUDGET_SESSION_USD", "30.0"))
    abort_at = budget_session * 0.85

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = RUNS_DIR / timestamp
    run_dir.mkdir(parents=True, exist_ok=True)
    print(f"runs/: {run_dir.relative_to(REPO_ROOT)}")
    print(f"budget: ${budget_session:.2f} session, ${abort_at:.2f} abort threshold")

    _, kde_mcp_arm, vision_arm = _make_clients_and_arms()
    arms = {"kde-mcp": kde_mcp_arm, "vision": vision_arm}
    client = Anthropic(api_key=api_key)

    classifications: dict[tuple[str, str, int], str] = {}
    total_spend = 0.0

    def one_trial(code: str, arm_name: str, trial: int) -> str:
        nonlocal total_spend
        scenario = load_scenario(SCENARIOS_DIR / f"{code}.yaml")
        log_path = run_dir / f"{code}-{arm_name}-{trial}.jsonl"
        with RunLogger(log_path, scenario=code, arm=arm_name, trial=trial) as log:
            scenario.setup(REPO_ROOT)
            try:
                result = run_loop(
                    scenario=scenario, arm=arms[arm_name], client=client,
                    logger=log, repo_root=REPO_ROOT,
                )
            finally:
                scenario.teardown()
        total_spend += result.total_usd
        (run_dir / "spend.json").write_text(json.dumps({"total_usd": total_spend}))
        print(f"  {code}-{arm_name}-{trial}: {result.classification} "
              f"(${result.total_usd:.4f}, {result.total_steps} steps, "
              f"session total ${total_spend:.4f})")
        if total_spend > abort_at:
            print(f"ABORT: session spend ${total_spend:.4f} > 0.85*budget ${abort_at:.4f}",
                  file=sys.stderr)
            raise SystemExit(3)
        return result.classification

    # Two trials each
    for code in ("E", "Q", "D", "R"):
        for arm_name in ("kde-mcp", "vision"):
            for trial in (1, 2):
                cls = one_trial(code, arm_name, trial)
                classifications[(code, arm_name, trial)] = cls

    # Tiebreak: any (scenario × arm) with disagreement on pass/fail axis?
    for code in ("E", "Q", "D", "R"):
        for arm_name in ("kde-mcp", "vision"):
            c1 = classifications[(code, arm_name, 1)]
            c2 = classifications[(code, arm_name, 2)]
            passed1 = (c1 == "pass")
            passed2 = (c2 == "pass")
            if passed1 != passed2:
                print(f"tiebreak: {code}-{arm_name} (trial 1={c1}, trial 2={c2})")
                c3 = one_trial(code, arm_name, 3)
                classifications[(code, arm_name, 3)] = c3

    print("\nrun-all complete")
    emit_summary(run_dir, classifications, total_spend)
    return 0


def _report(runs_dir: Path) -> int:
    """Re-emit summary.md from a runs/<timestamp>/ dir by replaying
    the .jsonl files."""
    from harness.report import emit_summary_from_logs
    emit_summary_from_logs(runs_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_cli.py -v`
Expected: 6 tests pass.

- [ ] **Step 5: Verify the entry point is wired**

Run: `cd harness && .venv/bin/harness --help`
Expected: argparse-formatted help text listing all 5 subcommands.

- [ ] **Step 6: Commit**

```bash
git add harness/src/harness/cli.py harness/tests/test_cli.py
git commit -m "feat(harness): CLI entry point — smoke, setup-policy, run(-all), report

Subcommands wire scenario loading, MCP client, arms, and loop into
one entry. setup-policy is the manual one-off for capturing the four
fixture classes; run-all is the orchestrator with tiebreaker logic
and session-budget abort. /healthz check at startup catches a dead
daemon early."
```

---

### Task 18: summary.md emitter

Aggregates per-trial classifications into the spec §4.12 summary table + per-scenario diagnosis section (which the diagnosis pass fills in later) + cost section.

**Files:**
- Create: `harness/src/harness/report.py`
- Create: `harness/tests/test_report.py`

- [ ] **Step 1: Write the failing test**

`harness/tests/test_report.py`:

```python
"""Tests for the summary emitter."""
import json
from pathlib import Path
from harness.report import emit_summary, render_summary


def test_render_summary_includes_section_headings():
    classifications = {("E", "kde-mcp", 1): "fail",
                       ("E", "kde-mcp", 2): "fail",
                       ("E", "vision", 1): "pass",
                       ("E", "vision", 2): "pass"}
    md = render_summary(classifications, total_spend=1.23)
    assert "# kde-mcp adversarial probe" in md
    assert "## Scope of inference" in md
    assert "## Summary table" in md
    assert "## Per-scenario diagnosis" in md
    assert "## Verdict" in md
    assert "## Cost" in md


def test_render_summary_table_renders_per_scenario_arm_counts():
    classifications = {
        ("E", "kde-mcp", 1): "fail", ("E", "kde-mcp", 2): "fail",
        ("E", "vision", 1): "pass", ("E", "vision", 2): "pass",
    }
    md = render_summary(classifications, total_spend=1.23)
    # E row: 0/2 pass on kde-mcp, 2/2 pass on vision
    assert "| E " in md
    assert "0/2" in md or "fail (2/2)" in md


def test_emit_summary_writes_file(tmp_path: Path):
    classifications = {("E", "kde-mcp", 1): "pass"}
    emit_summary(tmp_path, classifications, total_spend=0.5)
    assert (tmp_path / "summary.md").exists()


def test_render_summary_cost_block_includes_total():
    md = render_summary({}, total_spend=12.34)
    assert "$12.34" in md
```

- [ ] **Step 2: Run the test — expect failure (ImportError)**

Run: `cd harness && .venv/bin/pytest tests/test_report.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `harness/src/harness/report.py`**

```python
"""Summary.md emitter.

Per spec §4.12. Per-scenario diagnosis sections are placeholder-filled
("[fill from diagnosis pass — quote event ids]"); the diagnosis pass
(§4.11) edits the file by hand in an isolated session loaded
with only the logs + taxonomy excerpt.
"""
from __future__ import annotations
import json
from datetime import datetime, timezone
from pathlib import Path


SCOPE_DISCLAIMER = """\
This is a capability-boundary probe, not an average-workflow benchmark.
Each scenario isolates one theoretical weakness of text-only-on-AT-SPI;
the goal is to classify where and why each arm breaks. A reader who
wants to project these findings to their own workflows must check
whether those workflows hit the same boundaries.

n=2 per (scenario × arm), tiebreaker n=3 on pass/fail disagreement.
"""


def render_summary(
    classifications: dict[tuple[str, str, int], str],
    total_spend: float,
) -> str:
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    md = [
        f"# kde-mcp adversarial probe — {today}",
        "",
        "## Scope of inference",
        "",
        SCOPE_DISCLAIMER,
        "",
        "## Summary table",
        "",
        "| Scenario | Arm A (kde-mcp) | Arm B (vision) | Notes |",
        "|---|---|---|---|",
    ]

    for code in ("E", "Q", "D", "R"):
        a = _summarize_arm(classifications, code, "kde-mcp")
        b = _summarize_arm(classifications, code, "vision")
        md.append(f"| {code} | {a} | {b} | [fill from diagnosis pass] |")

    md += [
        "",
        "## Per-scenario diagnosis",
        "",
        "_Filled in an isolated session with only the JSONL logs and "
        "§4.10 taxonomy excerpt loaded. Every claim must quote an event id._",
        "",
    ]
    for code in ("E", "Q", "D", "R"):
        md += [
            f"### Scenario {code}",
            "",
            "[fill — 3 paragraphs, quoting event ids like `[E-vision-trial1-step-4]`]",
            "",
        ]

    md += [
        "## Bridge gaps observed",
        "",
        "[fill — list every `bridge_gap` event id from vision-arm logs, "
        "with action name and how the run recovered (or didn't)]",
        "",
        "## Verdict",
        "",
        "[fill — counts per classification × the §2 decision rule → "
        "concrete next-step recommendation]",
        "",
        "## Cost",
        "",
        f"| | Total |",
        f"|---|---:|",
        f"| Total spend | ${total_spend:.2f} |",
        "",
        f"_Per-arm and per-scenario cost breakdowns are in the trial "
        f"`run_end` events of the JSONL logs._",
        "",
    ]

    return "\n".join(md)


def _summarize_arm(
    classifications: dict[tuple[str, str, int], str],
    code: str,
    arm: str,
) -> str:
    trials = [v for (c, a, _), v in classifications.items() if c == code and a == arm]
    passes = sum(1 for v in trials if v == "pass")
    return f"{passes}/{len(trials)} pass; trials={trials}"


def emit_summary(
    run_dir: Path,
    classifications: dict[tuple[str, str, int], str],
    total_spend: float,
) -> None:
    md = render_summary(classifications, total_spend)
    (run_dir / "summary.md").write_text(md)


def emit_summary_from_logs(run_dir: Path) -> None:
    """Re-derive classifications from the .jsonl files in run_dir,
    then re-emit summary.md. Used by `harness report --runs ...`."""
    classifications: dict[tuple[str, str, int], str] = {}
    total_spend = 0.0
    for log_path in sorted(run_dir.glob("*.jsonl")):
        try:
            code, arm, trial_str = log_path.stem.split("-")
            trial = int(trial_str)
        except ValueError:
            continue
        for line in log_path.read_text().splitlines():
            rec = json.loads(line)
            if rec["event"] == "run_end":
                classifications[(code, arm, trial)] = rec["classification"]
                total_spend += rec.get("total_usd", 0.0)
    emit_summary(run_dir, classifications, total_spend)
```

- [ ] **Step 4: Run the test — expect pass**

Run: `cd harness && .venv/bin/pytest tests/test_report.py -v`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add harness/src/harness/report.py harness/tests/test_report.py
git commit -m "feat(harness): summary.md emitter

Per spec §4.12. Auto-fills the summary table from trial
classifications; leaves per-scenario diagnosis as placeholders for
the isolated-session diagnosis pass (§4.11). `harness report --runs ...`
re-emits the summary from saved JSONLs without re-running trials."
```

---

## Phase H — Execution & analysis

Three tasks: actually run the probes, draft the diagnosis in an isolated session, and dispatch the final cross-cutting review against the whole branch.

### Task 19: Execute the probe

This is the manual-but-documented step. After the user pre-curates `policy.toml`, the harness runs unattended for ~15-30 minutes.

- [ ] **Step 1: Ensure kde-mcp daemon is at HEAD**

```bash
cd ~/path/to/kde-mcp
cargo build --release
install -D -m 0755 target/release/kde-mcp ~/.local/bin/kde-mcp
systemctl --user restart kde-mcp-http.service
curl -s http://127.0.0.1:7575/healthz | jq
```

Expected: healthz returns `{"ok": true, "version": "0.1.0", "git_sha": "...", ...}` where git_sha matches `git rev-parse --short=12 HEAD` of the current branch.

- [ ] **Step 2: Verify ANTHROPIC_API_KEY**

```bash
echo "key length: ${#ANTHROPIC_API_KEY}"
```
Expected: > 50. If empty, set it: `export ANTHROPIC_API_KEY=sk-ant-...`.

- [ ] **Step 3: Re-run Phase 0 smoke**

```bash
cd ~/path/to/kde-mcp/harness
.venv/bin/harness smoke
```

Expected: `smoke: ok`. If anything else, STOP — re-resolve the failure mode before proceeding.

- [ ] **Step 4: Run setup-policy and edit policy.toml**

```bash
.venv/bin/harness setup-policy
```

Expected: four `[apps."kde-mcp-probe-*"]` snippets printed (plus `org.kde.gwenview` for R). Copy them into `~/.config/kde-mcp/policy.toml`:

```toml
# Add at the bottom; create the file if it doesn't exist
[apps."kde-mcp-probe-canvas"]
tier = "full"

[apps."kde-mcp-probe-qt-noname"]
tier = "full"

[apps."kde-mcp-probe-drag"]
tier = "full"

[apps."org.kde.gwenview"]
tier = "full"
```

Restart the daemon so it picks up the new policy: `systemctl --user restart kde-mcp-http.service`.

- [ ] **Step 5: Run all scenarios**

```bash
.venv/bin/harness run-all 2>&1 | tee /tmp/harness-run-all.log
```

Expected: 16 base runs + up to 4 tiebreakers; per-trial line printed (`E-kde-mcp-1: fail-essential ($0.0810, 8 steps, session total $0.0810)` etc.). Final line: `run-all complete`.

If a fixture window doesn't appear, or the daemon throws auth/policy errors mid-run — abort with Ctrl-C, inspect `harness/runs/<timestamp>/*.jsonl` and the daemon's audit log, fix, and re-run **fresh** (don't try to "resume" — every run-session is its own timestamp dir).

- [ ] **Step 6: Inspect outputs**

```bash
RUN_DIR=$(ls -td harness/runs/*/ | head -1)
ls "$RUN_DIR"        # 16 + tiebreaker jsonl files, summary.md, spend.json
cat "$RUN_DIR/spend.json"
cat "$RUN_DIR/summary.md" | head -40
```

Expected: `spend.json` shows `total_usd` between $8 and $18. `summary.md` has the auto-filled summary table with per-trial `passes/total` and placeholder diagnosis sections.

- [ ] **Step 7: Commit the run-session as a result artefact**

`harness/runs/` is gitignored. To preserve the result, copy the summary into `docs/`:

```bash
cp "$RUN_DIR/summary.md" docs/adversarial-probe-$(date -u +%Y-%m-%d).md
git add docs/adversarial-probe-$(date -u +%Y-%m-%d).md
git commit -m "docs: study 2 raw summary from $(date -u +%Y-%m-%d) run

Auto-emitted by 'harness run-all'. Per-scenario diagnosis sections
are placeholders; the next commit fills them via the diagnosis
protocol (§4.11)."
```

Don't commit the raw .jsonl logs — they may contain transient OS state. The summary suffices for downstream readers; if a future investigation needs the raw logs, they're in `harness/runs/<timestamp>/` locally.

---

### Task 20: Draft the diagnosis in an isolated session

Per spec §4.11, the diagnosis must be drafted in a fresh session loaded with only the logs + taxonomy excerpt. This isolates the diagnosis from the design's prior framing.

- [ ] **Step 1: Stage the inputs**

```bash
RUN_DIR=$(ls -td harness/runs/*/ | head -1)
DIAG_DIR=/tmp/diag-$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$DIAG_DIR"
cp "$RUN_DIR"/*.jsonl "$DIAG_DIR/"
```

Extract the §4.10 taxonomy into `"$DIAG_DIR/taxonomy.md"`:

```bash
sed -n '/### 4.10/,/### 4.11/p' \
    docs/specs/2026-05-19-evaluation-and-latency-design.md \
    > "$DIAG_DIR/taxonomy.md"
```

Copy the auto-emitted summary (with placeholders) as the starting draft:

```bash
cp "$RUN_DIR/summary.md" "$DIAG_DIR/summary.md"
```

- [ ] **Step 2: Open an isolated session in that dir**

```bash
cd "$DIAG_DIR"
claude
```

In the fresh session, send this prompt verbatim:

> I have the JSONL logs from an adversarial probe of a desktop-control MCP server (kde-mcp) against an alternative arm (Anthropic's computer-use). The taxonomy I must classify each (scenario × arm) outcome into is in taxonomy.md. The auto-emitted summary in summary.md has placeholder diagnosis sections — fill them. Every claim must quote a specific `event_id` from the JSONL logs (format `<scenario>-<arm>-trial<trial>-<event>[-n]`). Do not read any other file in this directory. Do not search the web. Do not assume context I haven't given you. Each per-scenario diagnosis is 3 paragraphs: what happened, why it happened (per the events), classification choice with reasoning.

- [ ] **Step 3: Review the draft**

Spot-check 3–5 quoted event ids by grep-ing the .jsonl files. If any quote doesn't match a real event, the diagnosis is hallucinating — re-prompt with the offending claim quoted back at it and ask for a corrected version.

- [ ] **Step 4: Move the diagnosed summary back into the repo**

```bash
cp "$DIAG_DIR/summary.md" docs/adversarial-probe-$(date -u +%Y-%m-%d).md
```

- [ ] **Step 5: Commit**

```bash
git add docs/adversarial-probe-$(date -u +%Y-%m-%d).md
git commit -m "docs: study 2 diagnosis drafted via §4.11 protocol

Isolated session loaded with only the JSONL logs and the §4.10
taxonomy excerpt. Every claim quotes an event_id. Verdict and
per-scenario diagnoses filled in; cost block matches spend.json."
```

---

### Task 21: Final cross-cutting code review

Per the Plan 1 lesson ([[feedback-cross-cutting-review-catches-contract-bugs]]), run one more cross-cutting review against the entire `feat/study-2-probe` branch before merging.

- [ ] **Step 1: Review the branch**

Review the entire `feat/study-2-probe` branch against this plan and the spec at `docs/specs/2026-05-19-evaluation-and-latency-design.md` §4. Specifically:

1. Field-name parity at every Python ↔ Rust boundary. JSON-RPC method names, kde-mcp tool arg shapes, Anthropic API field names. The Plan 1 ts/timestamp mismatch is the ur-example — find any analogue here.
2. Bridge-table completeness against src/tools/mouse.rs + src/tools/keyboard.rs. Every kde-mcp arg name used by a bridge entry must exist in the corresponding tool's Args struct in Rust.
3. Tiebreaker logic in cli._run_all — pass/fail axis only, not failure-mode (per spec §4.10).
4. Budget-guard: BUDGET_CAP_USD per trial AND BUDGET_SESSION_USD * 0.85 per session. Confirm both.
5. The diagnosis output (docs/adversarial-probe-*.md) actually has event-id quotes, not just narrative referencing 'the logs'.

Look for contract bugs, missing coverage, or any spec drift.

- [ ] **Step 2: Fix findings**

If the reviewer reports issues, fix each. Re-run the relevant tests. Commit each fix as its own commit with a clear message linking it to the review.

- [ ] **Step 3: Merge or open a PR**

Per project convention (CLAUDE.md): personal-use project, no remote push without explicit user ask. Merge locally:

```bash
git checkout main
git merge --no-ff feat/study-2-probe
```

(Or open a PR if the user requests one; do not push without explicit ask.)

---

## Self-review checklist


Run through this once after writing the plan:

- [ ] **Spec coverage:** §4.1 inference scope → diagnosis (Task 20) header copy. §4.2 fixtures → Tasks 13-16. §4.3 harness architecture → Tasks 4, 6-8, 9-12, 17-18. §4.4 loop pseudocode → Task 10. §4.5 arm A → Task 11. §4.6 arm B + bridge table → Task 12. §4.7 Phase 0 smoke → Task 5. §4.8 caps → Task 10 (per-trial) + Task 17 (per-session abort). §4.9 logging schema → Task 7 (logger) + Task 10 (event types). §4.10 taxonomy → Task 10 (loop classifications) + Task 20 (taxonomy.md staging). §4.11 diagnosis protocol → Task 20. §4.12 summary.md → Task 18. §4.13 operational concerns: row 1 (app classes) → Tasks 13-16 fixtures + Task 17 setup-policy. Row 2 (serial runs) → no explicit lock (single-binary CLI; document in README); row 3 (leftover-fixture-kill) → `_kill_existing` in scenario.py. Row 4 (cumulative budget) → Task 17 run-all. Row 5/6 (model + kde-mcp version) → Task 5 smoke captures model from scenario YAML + Task 19 step 1 captures /healthz. Row 7 (focus/geometry) → Tasks 13-16 fixtures sleep after launch. Row 8 (privacy) → harness/.gitignore. Row 9 (API key) → Task 5 smoke + Task 17 run-all check. Row 10 (KDE_MCP_SESSION_TAG) → Task 3 + Task 17 sets default. §5 cost projection → Task 6 pricing + Task 11 cache_control. §6 out-of-scope items → not implemented, correct.

- [ ] **No placeholders.** Every step has either runnable code, an exact command, or a manual verification with the expected outcome named. The placeholder text inside `summary.md` (`[fill from diagnosis pass]`) is itself the deliverable shape of Task 20 — replaced by the diagnosis pass.

- [ ] **Type consistency.** Anthropic `input_schema` (snake_case) consistent across `kde_mcp_arm.translate_tool_definitions` (Task 11), the Arm protocol (Task 10), and `Arm.tool_definitions()` (Tasks 11, 12). `tool_result` field shape (`tool_use_id`, `content`, `is_error`) consistent across loop (Task 10) and both arms (Tasks 11, 12). `BRIDGE_TABLE` entries all reference kde-mcp tool args that exist in `src/tools/mouse.rs` (action, button, count, coordinate, start_coordinate, scroll_direction, scroll_amount, duration_ms) and `src/tools/keyboard.rs` (text, keys, hold_ms) — verified during plan authoring. `event_id` format `<scenario>-<arm>-trial<trial>-<event>[-n]` consistent across `logger._make_event_id` (Task 7) and diagnosis prompt (Task 20). `classification` field values (`pass`, `fail`, `timeout-budget`, `timeout-steps`) consistent across loop return (Task 10), report aggregator (Task 18), and CLI tiebreaker logic (Task 17).

- [ ] **Contract-bug guards.** Live tests gated behind env vars: `HARNESS_LIVE_SMOKE` (Task 5), `HARNESS_LIVE_MCP` (Task 9). Cross-cutting review at each multi-component phase boundary (D, E, G, final). Plan 1 lesson incorporated.

---

## Execution notes

Recommended branch: `feat/study-2-probe`.
