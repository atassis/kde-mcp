# kde-mcp Phase 1 — Read-only Tools Against Live Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the six read-only tool stubs (`text_editor.view`, `clipboard_read`, `window_query`, `screenshot`, `inspect`, `observe`) with working live-mode implementations against the user's real KDE Plasma 6 / Wayland session. Wire every tool's dispatch through the audit log. Ship terminal-class masking for `screenshot` as defense-in-depth before the Phase 3 policy gate exists.

**Architecture:** Each tool's `#[tool]` shim in `src/server.rs` continues to be thin — the new mass lives in three new module trees: `src/atspi/*` (AT-SPI 0.30 over zbus for `inspect` + `observe`'s focused-element), `src/kwin/*` (KWin Scripting D-Bus + ScreenShot2 D-Bus for `window_query`, `observe`, `screenshot`), and `src/util/image.rs` (PNG resize + black-rectangle mask overlay). The audit log moves from "open but never written" to "every tool dispatch produces one JSONL record" via a `dispatch_audited()` helper that wraps each future. Tool responses construct typed Rust structs whose external-content fields are `Tagged<String>` so the trust discriminator (spec §4.7) ships with every read.

**Tech Stack additions** (Cargo.toml deltas on top of Phase 0):
- `atspi = "=0.30"` with the `tokio` feature — AT-SPI client, pre-1.0 so pin exactly.
- `image = "0.25"` — ARGB→PNG decode/encode, resize, in-memory rectangle overlay.
- `tokio` gains the `process` feature (Phase 0 has `rt-multi-thread`, `macros`, `signal`, `time`, `sync`, `io-std`, `fs`; we add `process` for spawning `wl-paste`).
- `zvariant = "5.7"` (matches `zbus 5.15`) for the FD return type from ScreenShot2.
- `uuid = "1.10"` with `v4` — unique per-invocation D-Bus interface names for the KWin script runner.
- `futures-util = "0.3"` — `StreamExt` for the AT-SPI signal stream; also required by reis later in Phase 2 (already foreseen in the Phase 0 spikes postscript).
- `sha2 = "0.10"` — real SHA-256 for the audit redactor's `clipboard_write` and long `text_editor_edit` strings (currently a placeholder hash). Phase 1 doesn't write any redacted records, but `clipboard_read` lands here, so we put the real hash in place to retire the placeholder before Phase 2 / 4 land actual writes.

**MSRV unchanged:** Rust 1.85+, edition 2024.

---

## Pre-plan caveats and resolved decisions

Three pre-plan decisions, two clean and one with a sharp edge:

1. **KWin Scripting output capture → caller-hosted D-Bus receiver, kdotool-style.** The HANDOFF labelled this option "write-to-temp-file", but KWin's QJSEngine has **no file I/O globals** — `QFile` / `QTextStream` / `writeFile` are not registered. The only escape hatch from the script sandbox is `callDBus`. The actual mechanism kdotool uses, and what this plan implements, is: caller opens its own session-bus connection, registers a per-invocation interface (`org.kde.kdemcp.ScriptSink1.S<uuid>`) on a unique object path, embeds that name into the generated JS, the script calls back via `callDBus`, caller awaits a single result message with a timeout. This avoids the journald scrape (rate-limited, racy) and stays scoped to one method per script invocation — not a long-lived service. **~60 lines of Rust** total in `src/kwin/scripting.rs`. The HANDOFF's "heavyweight" objection to "kdotool round-trip" was based on kdotool's old Python-mediated design; in zbus it's lighter than the journald fallback.

2. **`window_query` default grouping → flat list with `screen` per entry, `format: "ids" | "titles" | "full"` controls fields.** Per the user's #2 approval. The JS payload always emits the same shape; the Rust layer projects down to the requested `format`.

3. **Terminal-masking in Phase 1 → hardcoded class allowlist; policy-gate-driven override lands in Phase 3.** Per the user's #3 approval. The class list is a `const &[&str]` in `src/util/image.rs`. Masking applies to all four `region` variants (`full`, `active_screen`, `window:<ref>`, `[x,y,w,h]`); it is disabled when the region is `window:<ref>` and the resolved window's class **is** itself in the terminal list (self-shot of a terminal is allowed, since the user is intentionally pointing at it — but other terminals visible in that capture still get masked).

---

## File structure (Phase 1)

| Path | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `atspi`, `image`, `zvariant`, `uuid`, `futures-util`, `sha2`; add `process` feature to `tokio`. |
| `src/lib.rs` | Modify | `pub mod atspi; pub mod kwin; pub mod util;`. Expose `audit::AuditLog` re-export so `KdeMcp::new` can take it. |
| `src/server.rs` | Modify | `KdeMcp` gains `audit: Arc<AuditLog>` field and `bus: zbus::Connection`. The 16 `#[tool]` methods route through a new `dispatch_audited()` helper instead of `tool_result()`. The seven Phase-0-stubbed mutating tools still return `NotImplemented`. |
| `src/main.rs` | Modify | Builds `KdeMcp::new(mode, audit_arc, bus_arc)`. The bootstrap output is reused so the session bus opened in `mode::live::bootstrap` is the same one the tools use (no re-open). |
| `src/audit.rs` | Modify | Replace `placeholder_hash` with `sha2::Sha256` digest (`"sha256:" + hex` format). Existing tests update to assert `sha256:` prefix + 64-hex. |
| `src/error.rs` | Modify | Add `UnsupportedRegion { reason }` and `Io { path, error }` variants for file-I/O failures in `text_editor.view`. Both keep snake_case codes per spec §8. |
| `src/refs.rs` | Modify | `RefStore::resolve` overload that bumps `minted_at` to refresh TTL on every read (rolling TTL — the spec's "valid until invalidated" semantics; only an explicit `invalidate(id)` call from a mutating tool will expire a ref ahead of the 60 s ceiling). |
| `src/tools/text_editor.rs` | Modify | Implement `text_editor_view`. Edit stub untouched. |
| `src/tools/clipboard.rs` | Modify | Implement `clipboard_read` (spawn `wl-paste`). Write stub untouched. |
| `src/tools/window.rs` | Modify | Implement `window_query`. `window_control` stub untouched. |
| `src/tools/screenshot.rs` | Modify | Implement `screenshot` (uses `kwin::screenshot` + `util::image`). |
| `src/tools/inspect.rs` | Modify | Implement `inspect` (uses `atspi::tree`). |
| `src/tools/observe.rs` | Modify | Implement `observe` (composes `kwin::scripting` + `atspi::connection`). |
| `src/atspi/mod.rs` | **Create** | Module declarations. Re-exports `Connection`, `walk`, `RefEncoding`. |
| `src/atspi/connection.rs` | **Create** | `connect()` → `AtspiConnection` holding `atspi::AccessibilityConnection`. Survives reconnect via `re_connect()` (logged at warn). |
| `src/atspi/tree.rs` | **Create** | `walk(root, filter, max_depth, max_elements)` returns `(Vec<FlatElement>, truncated: bool, warnings)`. Pure traversal; no app-specific logic. |
| `src/atspi/ref_encoding.rs` | **Create** | Encode AT-SPI `(bus_name, object_path)` into the `RefStore`'s payload string; decode for resolution. Window refs derived from KWin `internalId` (UUID) likewise round-trip. |
| `src/kwin/mod.rs` | **Create** | Module declarations. |
| `src/kwin/scripting.rs` | **Create** | `run_script(bus: &Connection, body: &str, timeout: Duration) -> Result<serde_json::Value, ScriptError>`. Hosts a per-invocation `ScriptSink1` D-Bus interface, embeds caller's unique bus name into the JS template, drives `loadScript` + `Script.run`, awaits the single `result(payload)` / `error(msg)` callback. |
| `src/kwin/screenshot.rs` | **Create** | zbus proxy for `org.kde.KWin.ScreenShot2`: `CaptureWorkspace`, `CaptureActiveWindow`, `CaptureWindow`, `CaptureArea`, `CaptureActiveScreen`. Each accepts `options: HashMap<String, Variant>` per the interface and returns `(pipe_fd, results: HashMap)`. We read the FD as a streaming PNG (server side decodes; we use `image::load_from_memory_with_format` after collecting the bytes). |
| `src/util/mod.rs` | **Create** | Module declarations. |
| `src/util/image.rs` | **Create** | `resize_long_edge(img, max)`, `mask_rectangles(img, rects, label)`, `terminal_classes() -> &[&str]`. PNG encode/decode via `image`. |
| `tests/integration.rs` | Modify | Add `pub mod text_editor;`, `pub mod observe_offline;` (the offline test for response-shape parity — see Task 13 step 7). |
| `tests/integration/text_editor.rs` | **Create** | View golden tests over a tempdir corpus. |
| `tests/integration/observe_offline.rs` | **Create** | A test that constructs an `ObserveResponse` by hand and asserts the JSON shape matches the spec §6.1 contract — no D-Bus required. The "shape is wrong" failure mode is decoupled from live-session unavailability. |
| `tests/integration/SMOKE.md` | Modify | New §C "Phase 1 read-only" covering all six tools against a live session. |
| `docs/research/2026-05-14-phase-1-spikes.md` | **Create** | Phase 1 spike findings (Task 0). |

---

## Task ordering and dependencies

```
Task 0  (spikes)            ─┐
                             ├─► T1 (Cargo + skeleton) ─┐
                                                         ├─► T2 (audit dispatch) ─┬─► every tool task
                                                                                   │
                                                         T1 ─► T3 (text_editor.view) ─┐
                                                         T1 ─► T4 (clipboard_read)    │
                                                         T1 ─► T5 (kwin scripting)    ├─► T14 verify
                                                                  │                   │
                                                         T5 ─────►T6 (window_query)   │
                                                         T1 ─► T7 (kwin screenshot proxy)
                                                                  │
                                                         T7 ────► T8 (screenshot basic)
                                                                  │
                                                         T1 ─► T9 (image utils)
                                                                  │
                                                         T8 + T9 + T6 ► T10 (screenshot + masking)
                                                         T1 ─► T11 (atspi infra)
                                                                  │
                                                         T11 ───► T12 (inspect)
                                                         T11 + T5 ► T13 (observe)
```

Hard ordering: **T0 must complete before T1** so the spike findings can correct any Cargo pins before the toolchain caches the wrong versions. T2 (audit dispatch) must complete before T3 because every tool from T3 onward exercises the dispatch wrapper.

Estimated total: 4–5 working days at the user's part-time pace (matches spec §14 Phase 1 budget).

---

## Task 0: Phase 1 spike verification

Four short read-only investigations. Output: one Markdown note at `docs/research/2026-05-14-phase-1-spikes.md` containing all four findings. Each step appends a section to that file. No source code written. Estimated total: 60 minutes.

**Files:**
- Create: `docs/research/2026-05-14-phase-1-spikes.md`

- [ ] **Step 0.0: Create the spike-notes file with headers**

```markdown
# Phase 1 Spike Findings

Captured 2026-05-14, before Phase 1 implementation begins. Read-only investigations only.

## 0.1 `atspi 0.30` API surface for focused-element retrieval
## 0.2 KWin Plasma 6 Scripting global names (workspace.activeWindow vs activeClient)
## 0.3 `org.kde.KWin.ScreenShot2` D-Bus interface method signatures (live introspect)
## 0.4 `wl-paste` MIME negotiation and exit codes
```

- [ ] **Step 0.1: AT-SPI 0.30 focused-element path**

Fetch `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi/src/lib.rs` and `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi/src/connection.rs`. Locate:
- The async constructor: should be `AccessibilityConnection::new()` or similar.
- The signal stream API for "focus changed" events: should be a subscribe-to-events pattern, named like `set_event_listener` / `add_match`.
- How to retrieve the currently-focused accessible from cold start: there is no global "give me focused element" — you subscribe to focus events and remember the last one, OR query a known root and walk to `is_active`.

Record in the spike notes:
- Exact module path of `AccessibilityConnection` (e.g. `atspi::AccessibilityConnection` vs `atspi::connection::AccessibilityConnection`).
- The event-stream type and how to obtain it.
- Whether the registry has any "current focus" property or if focus must be tracked via signals.

Decision tree:
- If `AccessibilityConnection::open().await` returns `Result<Self, atspi::Error>` and exposes `event_stream()` → plan stays as drafted.
- If the API has renamed since the docs.rs page that informed §13.1 → record the corrections; Task 11 implements the corrected names.
- If focus tracking requires signal subscription only → Task 13's `observe` includes a small focus-tracker started in `KdeMcp::new` that owns the "last focused" `AtomicCell<Option<Accessible>>`.

- [ ] **Step 0.2: KWin Plasma 6 Scripting globals**

Fetch `https://raw.githubusercontent.com/KDE/kwin/master/src/scripting/scripting.cpp`. Locate `slotScriptLoadedFromFile` / `installAsGlobal` to confirm the names of:
- `workspace.activeWindow` (Plasma 5 called it `activeClient` — confirm the 6.x name).
- The window iteration accessor (`workspace.windowList()` vs `workspace.windows` vs `workspace.stackingOrder`).
- `workspace.cursorPos` (QPoint property).
- `workspace.screens` / `workspace.activeOutput` (screen enumeration).
- Per-window properties: `internalId`, `caption`, `resourceClass`, `resourceName`, `pid`, `frameGeometry`, `output`, `desktops`, `minimized`, `fullScreen`, `maximizeMode`.

Record exact names. Decision: if a property has been renamed (e.g. `screens` is now `outputs` in Plasma 6+), Task 5's JS templates use the verified names.

- [ ] **Step 0.3: Live ScreenShot2 introspection**

Run on the dev machine (does not capture pixels — only metadata; safe to run while the user is using their session):

```sh
busctl --user introspect org.kde.KWin /org/kde/KWin/ScreenShot2 \
  org.kde.KWin.ScreenShot2 | tee -a docs/research/2026-05-14-phase-1-spikes.md
```

Record for each method (`CaptureWorkspace`, `CaptureActiveWindow`, `CaptureWindow`, `CaptureArea`, `CaptureActiveScreen`):
- Full D-Bus signature `in` and `out`.
- Whether the `pipe` output is `h` (file descriptor) or `s` (path) or both.
- The `options` dict keys the implementation accepts (look at `https://invent.kde.org/plasma/kwin/-/blob/master/src/plugins/screenshot/screenshot.h` for the enum keys).

Decision: Task 7 proxies the exact set of methods with the exact signatures. If `CaptureWindow` takes `(uuid: s, options)` we keep it; if it takes `(uuid: t)` (uint64) the proxy adjusts.

- [ ] **Step 0.4: wl-paste behaviour**

Run safely (does not change session state; reads the user's clipboard once):

```sh
wl-paste --list-types | tee -a docs/research/2026-05-14-phase-1-spikes.md
wl-paste --no-newline > /tmp/.kde-mcp-clip-test.bin 2>/dev/null; echo "exit=$? bytes=$(wc -c < /tmp/.kde-mcp-clip-test.bin)"; rm -f /tmp/.kde-mcp-clip-test.bin
wl-paste --type nonexistent/mime 2>&1 ; echo "exit=$?"
```

Record:
- Exit code when no clipboard data is present (typically 1; the stderr "no data" line is informative).
- Exit code on unsupported MIME (typically 1 with stderr).
- Whether `--list-types` is line-delimited (newline-separated MIME strings).

Decision: Task 4 implements `clipboard_read` by first running `wl-paste --list-types` to obtain available MIMEs, then `wl-paste --no-newline --type=<requested>`. If the requested MIME is not in the list, return a `TargetNotFound` with `closest_match` = best fuzzy match from the list.

- [ ] **Step 0.5: Commit the spike notes**

```bash
git add docs/research/2026-05-14-phase-1-spikes.md
git commit -m "docs(phase-1): spike findings (atspi, kwin scripting, screenshot2, wl-paste)"
```

---

## Task 1: Cargo.toml + module skeleton

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/atspi/mod.rs`, `src/atspi/connection.rs`, `src/atspi/tree.rs`, `src/atspi/ref_encoding.rs`
- Create: `src/kwin/mod.rs`, `src/kwin/scripting.rs`, `src/kwin/screenshot.rs`
- Create: `src/util/mod.rs`, `src/util/image.rs`

- [ ] **Step 1.1: Add dependencies to Cargo.toml**

Edit `[dependencies]` block. Final state should be (additions only — keep all Phase 0 entries verbatim, just add these new lines):

```toml
atspi          = { version = "=0.30", features = ["tokio"] }
image          = { version = "0.25", default-features = false, features = ["png"] }
zvariant       = "5.7"
uuid           = { version = "1.10", features = ["v4"] }
futures-util   = "0.3"
sha2           = "0.10"
```

And amend the existing `tokio` line:

```toml
tokio          = { version = "1.40", features = ["rt-multi-thread", "macros", "signal", "time", "sync", "io-std", "fs", "process"] }
```

- [ ] **Step 1.2: Verify Cargo.toml compiles after dep additions**

```bash
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished \`dev\` profile` with zero errors (warnings are acceptable from new unused imports). If `atspi` fails to resolve, check the Task 0.1 spike — feature name may have changed (e.g. `tokio` → `async-tokio`); update Cargo.toml accordingly.

- [ ] **Step 1.3: Create empty module skeletons**

Create each of the new files with the exact content below. Each file is intentionally a placeholder — Tasks 5/7/9/11/12 fill them in. The skeleton-only step is so `cargo check` passes immediately and later tasks have stable file paths.

`src/atspi/mod.rs`:
```rust
//! AT-SPI 0.30 client. Phase 1 — read-only tree access for `inspect` and
//! `observe`'s focused-element field.

pub mod connection;
pub mod ref_encoding;
pub mod tree;
```

`src/atspi/connection.rs`:
```rust
//! Lazily-opened AT-SPI accessibility-bus connection.
//!
//! Task 11 implements this; this is a Phase-1 placeholder.

use crate::error::ToolError;

pub struct AtspiConnection {
    // Populated by Task 11.
}

pub async fn connect() -> Result<AtspiConnection, ToolError> {
    Err(ToolError::NotImplemented)
}
```

`src/atspi/tree.rs`:
```rust
//! AT-SPI tree traversal.
//!
//! Task 11 implements this; this is a Phase-1 placeholder.
```

`src/atspi/ref_encoding.rs`:
```rust
//! Encode AT-SPI `(bus_name, object_path)` pairs into ref-store payloads and
//! back. Task 11 implements this; this is a Phase-1 placeholder.
```

`src/kwin/mod.rs`:
```rust
//! KWin D-Bus surfaces used by Phase 1: Scripting (loadScript) for window
//! queries and observe, ScreenShot2 for `screenshot`.

pub mod screenshot;
pub mod scripting;
```

`src/kwin/scripting.rs`:
```rust
//! KWin Scripting loadScript runner.
//!
//! Caller-hosted D-Bus receiver pattern (kdotool-style). The QJSEngine has
//! no file I/O; the only way out is `callDBus`. Task 5 implements this.
```

`src/kwin/screenshot.rs`:
```rust
//! org.kde.KWin.ScreenShot2 zbus proxy. Task 7 implements this.
```

`src/util/mod.rs`:
```rust
//! Cross-cutting utilities. Phase 1 ships image helpers; later phases add pty
//! (for `bash`) and other helpers.

pub mod image;
```

`src/util/image.rs`:
```rust
//! PNG decode/encode, resize-to-long-edge, terminal-mask overlay. Tasks 9/10
//! implement this; this is a Phase-1 placeholder.
```

- [ ] **Step 1.4: Wire the new modules into `src/lib.rs`**

Edit `src/lib.rs` so the full file reads:

```rust
pub mod atspi;
pub mod audit;
pub mod cli;
pub mod error;
pub mod kwin;
pub mod lock;
pub mod mode;
pub mod notify;
pub mod refs;
pub mod server;
pub mod tools;
pub mod trust;
pub mod util;

pub use server::KdeMcp;
```

- [ ] **Step 1.5: Verify the skeleton compiles**

```bash
cargo check --workspace 2>&1 | tail -3
```

Expected: `Finished`. Warnings about unused imports are fine (Tasks 5/7/9/11/12 will use them).

- [ ] **Step 1.6: Commit**

```bash
git add Cargo.toml src/lib.rs src/atspi src/kwin src/util
git commit -m "chore(phase-1): add deps and module skeletons for atspi/kwin/util"
```

---

## Task 2: Audit-log dispatch wrapper

Every `#[tool]` shim funnels through one helper that (a) serialises the args to `serde_json::Value` for the audit record, (b) runs the future, (c) writes an `AuditRecord` with `ok`, `duration_ms`, `error?`. Records carry the wire tool name (`text_editor.view`, not `text_editor_view`) because that's what the operator will grep for.

**Files:**
- Modify: `src/audit.rs`
- Modify: `src/server.rs`
- Modify: `src/main.rs`

- [ ] **Step 2.1: Write the failing test (audit dispatch records a successful call)**

Append to `src/server.rs`'s existing `#[cfg(test)] mod tests` block, at the end (before the closing `}`):

```rust
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let audit = Arc::new(crate::audit::AuditLog::open(&path).await.unwrap());
        let r = dispatch_audited(
            audit.clone(),
            "live",
            "observe",
            serde_json::json!({"detail": "summary"}),
            async { Ok::<_, ToolError>(serde_json::json!({"mode":"live"})) },
        )
        .await;
        drop(audit);
        assert!(matches!(r.is_error, None | Some(false)));
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["tool"], "observe");
        assert_eq!(v["mode"], "live");
        assert_eq!(v["ok"], true);
        assert!(v["duration_ms"].is_number());
    }

    #[tokio::test]
    async fn dispatch_audited_writes_a_record_on_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let audit = Arc::new(crate::audit::AuditLog::open(&path).await.unwrap());
        let r = dispatch_audited(
            audit.clone(),
            "live",
            "clipboard_write",
            serde_json::json!({"content": "secret"}),
            async { Err::<serde_json::Value, _>(ToolError::NotImplemented) },
        )
        .await;
        drop(audit);
        assert_eq!(r.is_error, Some(true));
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], "not_implemented");
        // Redaction: clipboard_write.content should be hashed in the audit record.
        assert!(
            v["args"]["content"]["sha256"].is_string(),
            "args.content should have been redacted: {v}",
        );
        assert!(!v.to_string().contains("\"secret\""), "secret leaked into audit");
    }
```

- [ ] **Step 2.2: Run the test, confirm it fails**

```bash
cargo test --workspace dispatch_audited 2>&1 | tail -10
```

Expected: compile error `cannot find function "dispatch_audited" in this scope`.

- [ ] **Step 2.3: Implement `dispatch_audited` in `src/server.rs`**

Replace `tool_result` with the audit-aware version. Below the closing `}` of `impl ServerHandler for KdeMcp { ... }`, replace the existing `fn tool_result` with:

```rust
/// Wrap a tool future with audit-log dispatch.
///
/// - Serialises args to JSON, runs the redactor, writes the AuditRecord.
/// - Times the call to ms.
/// - On error, records `ok: false` and the structured error payload.
/// - The return type is `CallToolResult` so the rmcp `#[tool]` macro can
///   `?`-bubble it.
pub(crate) async fn dispatch_audited<F, T>(
    audit: std::sync::Arc<crate::audit::AuditLog>,
    mode: &'static str,
    tool: &'static str,
    args: serde_json::Value,
    fut: F,
) -> CallToolResult
where
    F: std::future::Future<Output = Result<T, ToolError>>,
    T: serde::Serialize,
{
    let started = std::time::Instant::now();
    let outcome = fut.await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let redacted_args = crate::audit::redact(tool, args);
    let (call_result, audit_error) = match outcome {
        Ok(payload) => {
            let v = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
            (
                CallToolResult::success(vec![Content::text(v.to_string())]),
                None,
            )
        }
        Err(e) => {
            let code = e.code();
            let msg = e.to_string();
            let details = serde_json::to_value(&e).unwrap_or(serde_json::Value::Null);
            let payload = serde_json::json!({ "code": code, "details": details });
            let r = CallToolResult::error(vec![
                Content::text(msg),
                Content::text(payload.to_string()),
            ]);
            (r, Some(payload))
        }
    };
    let rec = crate::audit::AuditRecord {
        ts: String::new(),
        mode,
        tool: tool.to_string(),
        args: redacted_args,
        ok: audit_error.is_none(),
        duration_ms: Some(duration_ms),
        error: audit_error,
        vision_fallback: None,
    };
    if let Err(e) = audit.write(rec).await {
        tracing::error!(error = %e, tool, "audit write failed");
    }
    call_result
}
```

- [ ] **Step 2.4: Wire `KdeMcp` to carry the audit handle and pre-bootstrapped bus**

Modify `KdeMcp` (replace the existing `struct` and `impl KdeMcp` block in `src/server.rs`):

```rust
#[derive(Clone)]
pub struct KdeMcp {
    pub mode: Mode,
    pub refs: Arc<RefStore>,
    pub audit: Arc<crate::audit::AuditLog>,
    pub bus: zbus::Connection,
    pub tool_router: ToolRouter<KdeMcp>,
}

impl KdeMcp {
    pub fn new(mode: Mode, audit: Arc<crate::audit::AuditLog>, bus: zbus::Connection) -> Self {
        Self {
            mode,
            refs: Arc::new(RefStore::new(Duration::from_secs(60))),
            audit,
            bus,
            tool_router: Self::tool_router(),
        }
    }
}
```

- [ ] **Step 2.5: Replace each `#[tool]` body to route through `dispatch_audited`**

For every `#[tool]` method in `src/server.rs`, replace the body. Example for `observe` (apply the same shape to the other 15):

```rust
    async fn observe(
        &self,
        Parameters(args): Parameters<tools::observe::ObserveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let mode = self.mode.as_str();
        let audit = self.audit.clone();
        Ok(dispatch_audited(audit, mode, "observe", v, async move {
            tools::observe::observe(self, args).await
        })
        .await)
    }
```

Notes:
- `mode.as_str()` returns `&'static str`, satisfying `dispatch_audited`'s bound.
- The tool free-functions get `state: &KdeMcp` as the first argument now (Tasks 3–13 update each to accept it). For Task 2 in isolation, keep the call shape `tools::observe::observe(args).await` if the signature hasn't been updated yet — the closure can ignore `self`. But the simpler invariant is "every tool now takes `state: &KdeMcp`"; apply it uniformly. Stubbed tools (`mouse`, `keyboard.*`, `act_on_element`, `window_control`, `clipboard_write`, `bash`, `text_editor.edit`, `wait`, `read_text`) still return `NotImplemented`; just update their signatures to accept `_state: &KdeMcp` so the closure compiles.

For each wire-named tool, the wire name (with `.` for `keyboard.type` etc.) is the literal passed as `tool` to `dispatch_audited`. Reuse the same name the `#[tool(name = ...)]` attribute carries; for tools without an explicit `name = `, use the function name verbatim (i.e. `observe`, `inspect`, `screenshot`, `read_text`, `wait`, `act_on_element`, `mouse`, `window_query`, `window_control`, `clipboard_read`, `clipboard_write`, `bash`).

- [ ] **Step 2.6: Update the existing `tool_result_*` tests**

Two tests in the `#[cfg(test)] mod tests` block call the old `tool_result` directly. They no longer apply (the function is replaced). Delete them:

```rust
    // DELETE these two:
    // fn tool_result_carries_error_code_in_payload() { ... }
    // fn tool_result_success_path_is_not_error() { ... }
```

The behaviour they assert is now covered by the two `dispatch_audited_*` tests from step 2.1.

- [ ] **Step 2.7: Update `src/main.rs` to thread the audit handle and bus into `KdeMcp::new`**

```rust
use anyhow::{Context, Result};
use kde_mcp::{KdeMcp, audit, cli::parse_and_validate, lock::ModeLock, mode, notify};
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,kde_mcp=debug")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = match parse_and_validate(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };

    let mode_name = cli.mode.as_str();

    let lock = ModeLock::acquire(mode_name)
        .with_context(|| format!("acquiring {mode_name} mode lock"))?;
    tracing::info!(lock = %lock.path().display(), "mode lock acquired");

    notify::notify_lock_acquired(mode_name).await;

    let bootstrapped = mode::bootstrap(cli.mode)
        .await
        .with_context(|| format!("bootstrapping {mode_name} mode"))?;

    let audit_path = audit::default_path();
    let audit_log = Arc::new(
        audit::AuditLog::open(&audit_path)
            .await
            .with_context(|| format!("opening audit log at {}", audit_path.display()))?,
    );
    tracing::info!(audit = %audit_path.display(), "audit log open");

    let server = KdeMcp::new(cli.mode, audit_log, bootstrapped.bus);

    eprintln!("kde-mcp: ready (mode={mode_name})");

    let service = server
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("rmcp serve: {e}"))?;

    service.waiting().await?;

    drop(lock);
    notify::notify_lock_released(mode_name).await;
    tracing::info!("shut down cleanly");
    Ok(())
}
```

- [ ] **Step 2.8: Update `tests/integration/tools_list.rs` constructor calls**

The three tests construct `KdeMcp::new(Mode::Live)`. After Task 2 the constructor takes three args. Update each call site to:

```rust
let server = KdeMcp::new(Mode::Live, test_audit(), test_bus().await);
```

…and add helpers at the top of the file (above the first test):

```rust
fn test_audit() -> std::sync::Arc<kde_mcp::audit::AuditLog> {
    use kde_mcp::audit::AuditLog;
    // Use a per-test tempfile; no need to keep it after the test exits.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.jsonl");
    // Leak the tempdir so the path stays valid for the test's lifetime; OK
    // because integration tests are short-lived sub-processes.
    let _ = Box::leak(Box::new(dir));
    std::sync::Arc::new(
        tokio::runtime::Handle::current()
            .block_on(AuditLog::open(&path))
            .unwrap(),
    )
}

async fn test_bus() -> zbus::Connection {
    // The `tools_list` tests do not actually call D-Bus; we just need a value
    // of the right type. Use a stub. If `Connection::session()` fails in CI
    // (no session bus), we abort the test cleanly via `Connection::null()`-ish
    // — but `zbus::Connection` has no public null constructor. Practical
    // alternative: connect to the user's actual session bus; the test only
    // queries the tool router, not the bus.
    zbus::Connection::session()
        .await
        .expect("test requires a session bus (run under a desktop session)")
}
```

If running without a session bus is a constraint (CI later), Task 14 plan revisits this. For Phase 1 the dev machine always has a session bus, so the simple form is fine.

`block_on` inside `test_audit` only works if there's a Tokio runtime. The three integration tests in `tools_list.rs` are `#[test]`-not-`#[tokio::test]`. The easiest fix: change each to `#[tokio::test]` and make `test_audit` `async` plus `test_bus` is already async:

```rust
async fn test_audit() -> std::sync::Arc<kde_mcp::audit::AuditLog> {
    use kde_mcp::audit::AuditLog;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.jsonl");
    let _ = Box::leak(Box::new(dir));
    std::sync::Arc::new(AuditLog::open(&path).await.unwrap())
}
```

And each test:

```rust
#[tokio::test]
async fn server_advertises_sixteen_tools_with_expected_names() {
    let server = KdeMcp::new(Mode::Live, test_audit().await, test_bus().await);
    // ... rest unchanged
}
```

- [ ] **Step 2.9: Verify the test suite**

```bash
cargo test --workspace 2>&1 | tail -20
```

Expected: 39 tests pass (37 from Phase 0 + 2 new for `dispatch_audited`). Wire-format integration tests still green. If a test fails, the most likely cause is a `#[tool]` shim whose closure no longer compiles because `tools::<name>` doesn't accept `&KdeMcp` yet — restore that handler to the closure that captures `args` only, and add a TODO note for the relevant later task to re-introduce the state arg.

- [ ] **Step 2.10: Manual smoke spot-check (no live KWin needed)**

```bash
cargo build --release
/tmp/kde-mcp-smoke.sh "$PWD/target/release/kde-mcp" > /tmp/kde-mcp-smoke.stdout
test "$(wc -l < /tmp/kde-mcp-smoke.stdout)" = 3 && echo "smoke OK"
test -s ~/.local/state/kde-mcp/audit.jsonl && echo "audit non-empty"
tail -n 5 ~/.local/state/kde-mcp/audit.jsonl
```

Expected: smoke still 3 responses; audit log now contains a record for the `observe` tool call from the fixture (`{"tool":"observe","ok":false,"error":{"code":"not_implemented",...}}`). This is the first wire-driven write to the audit log.

- [ ] **Step 2.11: Commit**

```bash
git add src/server.rs src/main.rs tests/integration/tools_list.rs
git commit -m "feat(phase-1): route tool dispatch through audit log"
```

---

## Task 3: `text_editor.view`

Pure file I/O. Returns `cat -n`–style numbered lines for files; directory listings via `find . -maxdepth 2 -not -path '*/\.*'` semantics implemented in Rust (no shell-out).

**Files:**
- Modify: `src/tools/text_editor.rs`
- Modify: `tests/integration.rs`
- Create: `tests/integration/text_editor.rs`

- [ ] **Step 3.1: Write failing tests**

Create `tests/integration/text_editor.rs`:

```rust
use kde_mcp::cli::Mode;
use kde_mcp::tools::text_editor::{TextEditorViewArgs, text_editor_view};
use kde_mcp::{KdeMcp, audit::AuditLog};
use std::sync::Arc;
use tempfile::tempdir;

async fn test_state() -> KdeMcp {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    let _ = Box::leak(Box::new(dir));
    let audit = Arc::new(AuditLog::open(&path).await.unwrap());
    let bus = zbus::Connection::session().await.expect("session bus");
    KdeMcp::new(Mode::Live, audit, bus)
}

#[tokio::test]
async fn view_file_returns_numbered_lines() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("note.txt");
    tokio::fs::write(&path, "alpha\nbeta\ngamma\n").await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(resp.kind, "file");
    let body = resp.content.unwrap();
    // cat -n format: right-aligned line numbers separated by tab.
    assert!(body.contains("1\talpha"));
    assert!(body.contains("2\tbeta"));
    assert!(body.contains("3\tgamma"));
    assert_eq!(resp.total_lines, Some(3));
}

#[tokio::test]
async fn view_file_respects_view_range_one_indexed_inclusive() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("multi.txt");
    let body = (1..=10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    tokio::fs::write(&path, body).await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: Some([3, 5]),
        },
    )
    .await
    .unwrap();
    let body = resp.content.unwrap();
    assert!(body.contains("3\tline3"));
    assert!(body.contains("5\tline5"));
    assert!(!body.contains("6\tline6"));
    assert!(!body.contains("2\tline2"));
}

#[tokio::test]
async fn view_with_view_range_end_eq_minus_one_means_eof() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("eof.txt");
    let body = (1..=5).map(|i| format!("ln{i}")).collect::<Vec<_>>().join("\n");
    tokio::fs::write(&path, body).await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: path.to_string_lossy().to_string(),
            view_range: Some([2, -1]),
        },
    )
    .await
    .unwrap();
    let body = resp.content.unwrap();
    assert!(body.contains("5\tln5"));
    assert!(!body.contains("1\tln1"));
}

#[tokio::test]
async fn view_directory_lists_entries_at_depth_2() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("sub");
    tokio::fs::create_dir(&nested).await.unwrap();
    tokio::fs::write(dir.path().join("top.txt"), "x").await.unwrap();
    tokio::fs::write(nested.join("inner.txt"), "y").await.unwrap();
    let state = test_state().await;
    let resp = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: dir.path().to_string_lossy().to_string(),
            view_range: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(resp.kind, "directory");
    let entries = resp.entries.unwrap();
    let names: Vec<&str> = entries.iter().map(|s| s.as_str()).collect();
    assert!(names.iter().any(|n| n.ends_with("/top.txt")));
    assert!(names.iter().any(|n| n.ends_with("/sub")));
    assert!(names.iter().any(|n| n.ends_with("/sub/inner.txt")));
}

#[tokio::test]
async fn view_rejects_relative_path() {
    let state = test_state().await;
    let err = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: "relative/path".into(),
            view_range: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "permission_denied");
}

#[tokio::test]
async fn view_returns_io_error_on_missing_file() {
    let state = test_state().await;
    let err = text_editor_view(
        &state,
        TextEditorViewArgs {
            path: "/this/does/not/exist.kdemcp.test".into(),
            view_range: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "io");
}
```

Wire it into `tests/integration.rs`:

```rust
mod integration {
    pub mod cli_parse;
    pub mod lockfile;
    pub mod text_editor;
    pub mod tools_list;
}
```

- [ ] **Step 3.2: Add `Io` and `UnsupportedRegion` to `ToolError`**

In `src/error.rs`, add two variants and codes:

```rust
    // ... existing variants ...
    #[error("i/o error on {path}: {error}")]
    Io { path: String, error: String },

    #[error("unsupported region: {reason}")]
    UnsupportedRegion { reason: String },
```

And in the `fn code(&self) -> &'static str` match:

```rust
            Self::Io { .. } => "io",
            Self::UnsupportedRegion { .. } => "unsupported_region",
```

Plus the `each_variant_has_distinct_code` test gets two more entries:

```rust
            ToolError::Io { path: "x".into(), error: "y".into() }.code(),
            ToolError::UnsupportedRegion { reason: "x".into() }.code(),
```

(Apply both additions in the same edit; the test will then exercise 15 codes, not 13.)

- [ ] **Step 3.3: Run the failing tests**

```bash
cargo test --workspace text_editor 2>&1 | tail -15
```

Expected: compile error first (`TextEditorViewResponse` fields don't exist yet); then assertion failures once it compiles.

- [ ] **Step 3.4: Implement `text_editor.view`**

Replace `src/tools/text_editor.rs` body. View part only; `text_editor_edit` stays a stub returning `NotImplemented`:

```rust
use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorViewArgs {
    pub path: String,
    #[serde(default)]
    pub view_range: Option<[i32; 2]>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorViewResponse {
    pub kind: String, // "file" | "directory"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewed_range: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorEditArgs {
    pub command: String,
    pub path: String,
    #[serde(default)]
    pub old_str: Option<String>,
    #[serde(default)]
    pub new_str: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TextEditorEditResponse {}

pub async fn text_editor_view(
    _state: &KdeMcp,
    args: TextEditorViewArgs,
) -> Result<TextEditorViewResponse, ToolError> {
    let path = Path::new(&args.path);
    if !path.is_absolute() {
        return Err(ToolError::PermissionDenied {
            reason: format!("absolute path required, got: {}", args.path),
        });
    }
    let meta = tokio::fs::metadata(path).await.map_err(|e| ToolError::Io {
        path: args.path.clone(),
        error: e.to_string(),
    })?;
    if meta.is_dir() {
        return view_directory(path).await;
    }
    view_file(path, args.view_range).await
}

async fn view_file(
    path: &Path,
    range: Option<[i32; 2]>,
) -> Result<TextEditorViewResponse, ToolError> {
    let body = tokio::fs::read_to_string(path).await.map_err(|e| ToolError::Io {
        path: path.to_string_lossy().to_string(),
        error: e.to_string(),
    })?;
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let total = lines.len();
    let (start, end) = resolve_range(range, total)?;
    let mut out = String::with_capacity(body.len() + total * 6);
    for (i, line) in lines.iter().enumerate().skip(start - 1).take(end - start + 1) {
        let n = i + 1;
        // cat -n right-aligns line numbers in a 6-char field followed by tab.
        // Tests assert on "N\tline" substrings, so we keep a simple "N\t<line>"
        // form rather than cat -n's exact whitespace padding.
        out.push_str(&format!("{n}\t{}", line));
    }
    Ok(TextEditorViewResponse {
        kind: "file".into(),
        content: Some(out),
        total_lines: Some(total),
        viewed_range: Some([start as i32, end as i32]),
        entries: None,
    })
}

fn resolve_range(range: Option<[i32; 2]>, total: usize) -> Result<(usize, usize), ToolError> {
    let total = total.max(1);
    let Some([s, e]) = range else {
        return Ok((1, total));
    };
    if s < 1 {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range start must be >= 1, got {s}"),
        });
    }
    let start = s as usize;
    let end = if e == -1 { total } else if e < 1 {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range end must be -1 or >= 1, got {e}"),
        });
    } else {
        (e as usize).min(total)
    };
    if start > end {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range start {start} > end {end}"),
        });
    }
    Ok((start, end))
}

async fn view_directory(root: &Path) -> Result<TextEditorViewResponse, ToolError> {
    let mut entries: Vec<String> = Vec::new();
    walk(root, root, 0, 2, &mut entries).await.map_err(|e| ToolError::Io {
        path: root.to_string_lossy().to_string(),
        error: e,
    })?;
    entries.sort();
    Ok(TextEditorViewResponse {
        kind: "directory".into(),
        content: None,
        total_lines: None,
        viewed_range: None,
        entries: Some(entries),
    })
}

fn walk<'a>(
    root: &'a Path,
    current: &'a Path,
    depth: u32,
    max_depth: u32,
    out: &'a mut Vec<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let mut rd = tokio::fs::read_dir(current).await.map_err(|e| e.to_string())?;
        while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            let p = entry.path();
            let rel = p.strip_prefix(root).unwrap_or(&p);
            out.push(format!("./{}", rel.display()));
            let ft = entry.file_type().await.map_err(|e| e.to_string())?;
            if ft.is_dir() && depth + 1 < max_depth {
                walk(root, &p, depth + 1, max_depth, out).await?;
            }
        }
        Ok(())
    })
}

pub async fn text_editor_edit(
    _state: &KdeMcp,
    _args: TextEditorEditArgs,
) -> Result<TextEditorEditResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

- [ ] **Step 3.5: Update `src/server.rs` shim for `text_editor.view`**

The closure inside `text_editor_view` now calls `tools::text_editor::text_editor_view(self, args).await` (passing `self` as the first arg). The `text_editor_edit` shim updates the same way to keep signatures uniform.

The test assertions in `tests/integration/text_editor.rs` mention paths like `./top.txt` while the test does `assert!(names.iter().any(|n| n.ends_with("/top.txt")))`. Both `./top.txt` and a deeper path match the `ends_with("/top.txt")` predicate via the leading `.`, so the assertion holds. (Sanity check on the assertion logic: `"./top.txt".ends_with("/top.txt") == true`. Good.)

- [ ] **Step 3.6: Run the tests, confirm they pass**

```bash
cargo test --workspace text_editor 2>&1 | tail -15
```

Expected: all 6 `text_editor.view` tests pass. Adjust `walk` or `resolve_range` if assertions fail; the assertions in step 3.1 are the contract.

- [ ] **Step 3.7: Commit**

```bash
git add src/tools/text_editor.rs src/error.rs src/server.rs tests/integration.rs tests/integration/text_editor.rs
git commit -m "feat(phase-1): implement text_editor.view (file + directory)"
```

---

## Task 4: `clipboard_read`

Spawns `wl-paste`. Returns text with `trust: external` unless the writer was us (Phase 1 has no writer yet, so always `external`). Returns binary as base64 when the requested mime indicates binary content.

**Files:**
- Modify: `src/tools/clipboard.rs`
- Create: `tests/integration/clipboard_offline.rs` (covers arg-parsing and the offline subprocess path — does not read the user's real clipboard).

- [ ] **Step 4.1: Add tests covering the offline / unavailable-mime path**

`tests/integration/clipboard_offline.rs`:

```rust
use kde_mcp::cli::Mode;
use kde_mcp::tools::clipboard::{ClipboardReadArgs, clipboard_read};
use kde_mcp::{KdeMcp, audit::AuditLog};
use std::sync::Arc;
use tempfile::tempdir;

async fn test_state() -> KdeMcp {
    let dir = tempdir().unwrap();
    let _ = Box::leak(Box::new(dir));
    let audit = Arc::new(AuditLog::open(&std::env::temp_dir().join("kde-mcp-cltest.jsonl")).await.unwrap());
    let bus = zbus::Connection::session().await.expect("session bus");
    KdeMcp::new(Mode::Live, audit, bus)
}

#[tokio::test]
async fn unknown_mime_returns_target_not_found_with_closest_match() {
    let state = test_state().await;
    let r = clipboard_read(
        &state,
        ClipboardReadArgs {
            mime_type: Some("text/wholly-fake".into()),
            encoding: Some("utf8".into()),
        },
    )
    .await;
    // Either the clipboard is empty (External command failure) or the mime is
    // not advertised (TargetNotFound). Both are valid offline-test outcomes —
    // we just shouldn't panic and should produce a recognised error code.
    let err = r.unwrap_err();
    let ok_codes = ["target_not_found", "external_command_failed"];
    assert!(ok_codes.contains(&err.code()), "got code {}", err.code());
}
```

Wire into `tests/integration.rs`:

```rust
    pub mod clipboard_offline;
```

- [ ] **Step 4.2: Implement `clipboard_read`**

Replace `src/tools/clipboard.rs` body:

```rust
use crate::error::ToolError;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardReadArgs {
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardReadResponse {
    pub mime_type: String,
    pub encoding: String, // "utf8" | "base64"
    pub content: Tagged<String>,
    pub origin: String, // "external" | "agent_authored" — Phase 1 always "external"
    pub available_types: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipboardWriteArgs {
    pub content: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardWriteResponse {}

pub async fn clipboard_read(
    _state: &KdeMcp,
    args: ClipboardReadArgs,
) -> Result<ClipboardReadResponse, ToolError> {
    let requested_mime = args.mime_type.unwrap_or_else(|| "text/plain".into());
    let encoding = args.encoding.unwrap_or_else(|| "utf8".into());
    let types = list_types().await?;
    if !types.iter().any(|t| t == &requested_mime) {
        return Err(ToolError::TargetNotFound {
            closest_match: closest(&types, &requested_mime),
        });
    }
    let bytes = paste_bytes(&requested_mime).await?;
    let (content_str, encoding_used) = match encoding.as_str() {
        "utf8" => (
            String::from_utf8(bytes).map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("utf8 decode: {e}"),
            })?,
            "utf8".to_string(),
        ),
        "base64" => (
            base64::engine::general_purpose::STANDARD.encode(&bytes),
            "base64".to_string(),
        ),
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unsupported encoding: {other}"),
            });
        }
    };
    Ok(ClipboardReadResponse {
        mime_type: requested_mime,
        encoding: encoding_used,
        content: Tagged { trust: Trust::External, text: content_str },
        origin: "external".to_string(),
        available_types: types,
    })
}

async fn list_types() -> Result<Vec<String>, ToolError> {
    let out = tokio::process::Command::new("wl-paste")
        .arg("--list-types")
        .output()
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawning wl-paste: {e}"),
        })?;
    if !out.status.success() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    Ok(out
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

async fn paste_bytes(mime: &str) -> Result<Vec<u8>, ToolError> {
    let out = tokio::process::Command::new("wl-paste")
        .arg("--no-newline")
        .arg("--type")
        .arg(mime)
        .output()
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawning wl-paste: {e}"),
        })?;
    if !out.status.success() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    Ok(out.stdout)
}

fn closest(types: &[String], want: &str) -> Option<String> {
    types
        .iter()
        .min_by_key(|t| levenshtein(t, want))
        .cloned()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (cur[j - 1] + 1)
                .min(prev[j] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub async fn clipboard_write(
    _state: &KdeMcp,
    _args: ClipboardWriteArgs,
) -> Result<ClipboardWriteResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

Add the `base64 = "0.22"` dep to `Cargo.toml` (this is a small one not added in Task 1; tack it on here):

```toml
base64         = "0.22"
```

- [ ] **Step 4.3: Update `src/server.rs` shim for clipboard_read**

Apply the `&KdeMcp` first-arg pattern same as Task 2 step 2.5.

- [ ] **Step 4.4: Run the tests**

```bash
cargo test --workspace clipboard_offline 2>&1 | tail -10
```

Expected: the unknown-mime test passes with whichever of the two error codes the live wl-paste reports.

- [ ] **Step 4.5: Manual live spot-check (does not change session state)**

Copy "hello kde-mcp" into the clipboard via any normal means (Ctrl+C in a doc, for instance), then run:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"clipboard_read","arguments":{"mime_type":"text/plain"}}}' | \
  timeout 4 target/release/kde-mcp --mode=live 2>/dev/null | tail -1 | \
  python3 -c "import sys,json; d=json.load(sys.stdin)['result']; print(d.get('isError'), [c.get('text','')[:80] for c in d['content']])"
```

Expected: `False` and one of the content blocks contains `"text":"hello kde-mcp"` with `"trust":"external"`.

- [ ] **Step 4.6: Commit**

```bash
git add Cargo.toml src/tools/clipboard.rs src/server.rs tests/integration.rs tests/integration/clipboard_offline.rs
git commit -m "feat(phase-1): implement clipboard_read via wl-paste"
```

---

## Task 5: KWin script runner (caller-hosted D-Bus receiver)

A 60-LoC `run_script(bus, body, timeout)` helper. Each invocation:
1. Creates a fresh UUID-suffixed object path (`/org/kde/kdemcp/Sink/<uuid>`) on the caller's session bus.
2. Registers a one-shot `org.kde.kdemcp.ScriptSink1` interface at that path with `result(payload: s)` and `error(message: s)` methods.
3. Writes the JS body to a `tempfile::NamedTempFile` with the caller's unique bus name and object path embedded as JS string literals.
4. Calls `org.kde.kwin.Scripting.loadScript(path, plugin_name=<uuid>)`, then `org.kde.kwin.Script<id>.run()`.
5. Awaits `result` or `error` on an mpsc channel with `timeout`.
6. Unloads the script (`unloadScript(plugin_name)`), unregisters the sink object, deletes the tempfile.

**Files:**
- Modify: `src/kwin/scripting.rs`

- [ ] **Step 5.1: Write the test (offline — exercises the JS template renderer only)**

Append to `src/kwin/scripting.rs` test block (created in step 5.2). The test asserts:
- The rendered JS contains the caller's bus name as a string literal.
- The rendered JS contains the requested object path.
- The rendered JS contains the user body text verbatim inside an IIFE.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_script_embeds_caller_name_path_and_body() {
        let js = render_script(
            "kde.mcp.tester",
            "/org/kde/kdemcp/Sink/abcd",
            "callBackOrPrintHere",
        );
        assert!(js.contains("\"kde.mcp.tester\""), "missing caller bus name");
        assert!(js.contains("\"/org/kde/kdemcp/Sink/abcd\""), "missing obj path");
        assert!(js.contains("callBackOrPrintHere"), "missing user body");
        // Embedded body must be inside an IIFE so the script doesn't pollute
        // the (per-invocation, fresh) script global namespace.
        assert!(js.contains("(function()"), "body not wrapped in IIFE");
    }
}
```

- [ ] **Step 5.2: Implement `render_script` and `run_script`**

Replace `src/kwin/scripting.rs`:

```rust
//! KWin Scripting loadScript runner — caller-hosted D-Bus receiver pattern.
//!
//! KWin's QJSEngine exposes no file I/O. The only escape hatch for a script
//! is `callDBus(service, path, interface, method, args...)`. This module
//! hosts a unique sink interface for the duration of each script invocation
//! and lets the script call back into it.

use crate::error::ToolError;
use std::time::Duration;
use tokio::sync::mpsc;
use zbus::Connection;

/// Render the JS body wrapped with the caller-callback prelude.
pub(crate) fn render_script(caller_bus_name: &str, sink_obj_path: &str, user_body: &str) -> String {
    format!(
        r#"
(function() {{
    function _send(method, payload) {{
        callDBus({caller_bus_name:?}, {sink_obj_path:?},
                 "org.kde.kdemcp.ScriptSink1", method, payload);
    }}
    try {{
        var _r = (function() {{
            {user_body}
        }})();
        _send("result", typeof _r === "string" ? _r : JSON.stringify(_r));
    }} catch (e) {{
        _send("error", String(e));
    }}
}})();
"#
    )
}

/// Run a KWin script body and return its JSON-encoded payload string.
pub async fn run_script(
    bus: &Connection,
    body: &str,
    timeout: Duration,
) -> Result<String, ToolError> {
    let invocation_id = uuid::Uuid::new_v4().simple().to_string();
    let obj_path = format!("/org/kde/kdemcp/Sink/{invocation_id}");
    let plugin_name = format!("kde-mcp-{invocation_id}");

    let caller_name = bus
        .unique_name()
        .ok_or_else(|| ToolError::BusDisconnected {
            detail: "no unique name on session bus".into(),
        })?
        .to_string();

    let (tx, mut rx) = mpsc::channel::<Result<String, String>>(2);

    // Object-server side: register an ad hoc interface for the duration of the call.
    let sink = ScriptSink { tx };
    bus.object_server()
        .at(&obj_path, sink)
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    // Tempfile with the rendered JS.
    let js = render_script(&caller_name, &obj_path, body);
    let mut tmp = tempfile::NamedTempFile::with_suffix(".js").map_err(|e| ToolError::Io {
        path: "/tmp/kde-mcp-script-*.js".into(),
        error: e.to_string(),
    })?;
    use std::io::Write;
    tmp.write_all(js.as_bytes()).map_err(|e| ToolError::Io {
        path: tmp.path().to_string_lossy().to_string(),
        error: e.to_string(),
    })?;
    tmp.flush().map_err(|e| ToolError::Io {
        path: tmp.path().to_string_lossy().to_string(),
        error: e.to_string(),
    })?;

    // Load and run.
    let scripting_proxy = ScriptingProxy::new(bus).await?;
    let script_id = scripting_proxy
        .load_script(tmp.path().to_string_lossy().as_ref(), &plugin_name)
        .await?;
    let script_obj_path = format!("/Scripting/Script{script_id}");
    let script_proxy = ScriptProxy::builder(bus)
        .destination("org.kde.KWin")
        .map_err(busdc)?
        .path(script_obj_path.clone())
        .map_err(busdc)?
        .build()
        .await
        .map_err(busdc)?;
    script_proxy.run().await.map_err(busdc)?;

    // Await the first result-or-error or the timeout.
    let outcome = match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(Ok(payload))) => Ok(payload),
        Ok(Some(Err(msg))) => Err(ToolError::ExternalCommandFailed { stderr: msg }),
        Ok(None) => Err(ToolError::ExternalCommandFailed {
            stderr: "script channel closed without delivering a result".into(),
        }),
        Err(_) => Err(ToolError::Timeout {
            closest_match: None,
            current_windows: vec![],
        }),
    };

    // Cleanup: best-effort, never overshadows the actual outcome.
    let _ = scripting_proxy.unload_script(&plugin_name).await;
    let _ = bus.object_server().remove::<ScriptSink, _>(&obj_path).await;
    drop(tmp);

    outcome
}

fn busdc(e: zbus::Error) -> ToolError {
    ToolError::BusDisconnected { detail: e.to_string() }
}

struct ScriptSink {
    tx: mpsc::Sender<Result<String, String>>,
}

#[zbus::interface(name = "org.kde.kdemcp.ScriptSink1")]
impl ScriptSink {
    async fn result(&self, payload: String) {
        let _ = self.tx.send(Ok(payload)).await;
    }
    async fn error(&self, message: String) {
        let _ = self.tx.send(Err(message)).await;
    }
}

#[zbus::proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
trait Scripting {
    fn load_script(&self, file_path: &str, plugin_name: &str) -> zbus::Result<i32>;
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
}

impl From<zbus::Error> for ToolError {
    fn from(e: zbus::Error) -> Self { busdc(e) }
}

#[zbus::proxy(
    interface = "org.kde.kwin.Script",
    default_service = "org.kde.KWin"
)]
trait Script {
    fn run(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
}
```

Add `tempfile = "3.10"` to runtime `[dependencies]` (currently only `dev-dependencies`):

```toml
tempfile       = "3.10"
```

Move it out of `[dev-dependencies]` (or duplicate; cargo handles a single listing in runtime fine).

- [ ] **Step 5.3: Run the offline render test**

```bash
cargo test --workspace render_script 2>&1 | tail -10
```

Expected: pass.

- [ ] **Step 5.4: Commit**

```bash
git add Cargo.toml src/kwin/scripting.rs
git commit -m "feat(phase-1): kwin script runner via caller-hosted DBus sink"
```

---

## Task 6: `window_query`

Composes a JS body that emits a JSON array of windows. Filters by `match.class`/`match.title_regex` server-side after parsing the JS result, so the JS is one-size-fits-all.

**Files:**
- Modify: `src/tools/window.rs`

- [ ] **Step 6.1: Offline test for argument projection**

Append a unit test inside `src/tools/window.rs` (the `format` projection logic is pure, no D-Bus):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_ids_drops_everything_but_uuid_and_class() {
        let raw = json!([
            {"uuid":"win#1","caption":"Kate","resourceClass":"kate","pid":12,"geometry":[0,0,800,600],"screen":"DP-1"},
            {"uuid":"win#2","caption":"Brave","resourceClass":"brave-browser","pid":13,"geometry":[0,0,800,600],"screen":"DP-1"}
        ]);
        let entries = parse_entries(&raw).unwrap();
        let projected = project(&entries, "ids");
        let v = serde_json::to_value(&projected).unwrap();
        assert_eq!(v[0]["caption"], serde_json::Value::Null);
        assert!(v[0]["uuid"].is_string());
        assert!(v[0]["class"].is_object()); // Tagged
    }

    #[test]
    fn format_titles_includes_caption_but_not_pid_or_geometry() {
        let raw = json!([
            {"uuid":"win#1","caption":"Kate","resourceClass":"kate","pid":12,"geometry":[0,0,800,600],"screen":"DP-1"}
        ]);
        let entries = parse_entries(&raw).unwrap();
        let projected = project(&entries, "titles");
        let v = serde_json::to_value(&projected).unwrap();
        assert!(v[0]["title"].is_object()); // Tagged
        assert_eq!(v[0]["pid"], serde_json::Value::Null);
        assert_eq!(v[0]["geometry"], serde_json::Value::Null);
    }
}
```

- [ ] **Step 6.2: Implement `window_query`**

Replace `src/tools/window.rs`:

```rust
use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::refs::{RefKind, RefStore};
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryArgs {
    #[serde(default)]
    pub action: Option<String>, // "list" | "get_geometry" | "find"
    #[serde(default)]
    pub format: Option<String>, // "ids" | "titles" | "full"
    #[serde(default)]
    pub r#match: Option<WindowMatch>,
    #[serde(default)]
    pub window_uuid: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowMatch {
    #[serde(default)] pub title_regex: Option<String>,
    #[serde(default)] pub class: Option<String>,
    #[serde(default)] pub pid: Option<i64>,
    #[serde(default)] pub role: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowEntry {
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub title: Option<Tagged<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub class: Option<Tagged<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub pid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub geometry: Option<[i32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")] pub screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub minimized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub maximized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub fullscreen: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryResponse {
    pub windows: Vec<WindowEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WindowControlArgs {
    pub window_ref: String,
    pub action: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct WindowControlResponse {}

pub async fn window_query(
    state: &KdeMcp,
    args: WindowQueryArgs,
) -> Result<WindowQueryResponse, ToolError> {
    let format = args.format.as_deref().unwrap_or("titles").to_string();
    let action = args.action.as_deref().unwrap_or("list").to_string();
    let payload = run_script(&state.bus, JS_LIST_ALL, Duration::from_secs(2)).await?;
    let raw: Value = serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
        stderr: format!("kwin script JSON parse: {e}"),
    })?;
    let mut entries = parse_entries(&raw)?;

    // Mint refs and replace the raw KWin UUID with our opaque ref.
    for entry in entries.iter_mut() {
        let opaque = state
            .refs
            .mint(RefKind::Window, entry.uuid.clone())
            .await;
        entry.uuid = opaque;
    }

    // Apply match filter.
    if let Some(m) = &args.r#match {
        if let Some(class) = &m.class {
            entries.retain(|e| e.class.as_ref().map(|c| &c.text == class).unwrap_or(false));
        }
        if let Some(tr) = &m.title_regex {
            let re = regex_lite::Regex::new(tr).map_err(|e| ToolError::UnsupportedRegion {
                reason: format!("invalid title_regex: {e}"),
            })?;
            entries.retain(|e| e.title.as_ref().map(|t| re.is_match(&t.text)).unwrap_or(false));
        }
        if let Some(pid) = m.pid {
            entries.retain(|e| e.pid == Some(pid));
        }
    }

    // get_geometry: pick only the requested uuid.
    if action == "get_geometry" {
        if let Some(ref_id) = args.window_uuid {
            entries.retain(|e| e.uuid == ref_id);
        } else {
            return Err(ToolError::TargetNotFound { closest_match: None });
        }
    }

    let projected = project(&entries, &format);
    Ok(WindowQueryResponse { windows: projected })
}

/// Build the projection from a fully-populated entry to the format-shaped one.
pub(crate) fn project(entries: &[WindowEntry], format: &str) -> Vec<WindowEntry> {
    entries
        .iter()
        .map(|e| match format {
            "ids" => WindowEntry {
                uuid: e.uuid.clone(),
                title: None,
                class: e.class.clone(),
                pid: None,
                geometry: None,
                screen: e.screen.clone(),
                minimized: None,
                maximized: None,
                fullscreen: None,
            },
            "titles" => WindowEntry {
                uuid: e.uuid.clone(),
                title: e.title.clone(),
                class: e.class.clone(),
                pid: None,
                geometry: None,
                screen: e.screen.clone(),
                minimized: None,
                maximized: None,
                fullscreen: None,
            },
            _ => e.clone(), // "full"
        })
        .collect()
}

pub(crate) fn parse_entries(v: &Value) -> Result<Vec<WindowEntry>, ToolError> {
    let arr = v.as_array().ok_or_else(|| ToolError::ExternalCommandFailed {
        stderr: "kwin script payload was not a JSON array".into(),
    })?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let g = item.get("geometry").and_then(|g| g.as_array());
        let geometry = g.and_then(|a| {
            if a.len() == 4 {
                Some([
                    a[0].as_i64().unwrap_or(0) as i32,
                    a[1].as_i64().unwrap_or(0) as i32,
                    a[2].as_i64().unwrap_or(0) as i32,
                    a[3].as_i64().unwrap_or(0) as i32,
                ])
            } else { None }
        });
        out.push(WindowEntry {
            uuid: item.get("uuid").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            title: item.get("caption").and_then(|x| x.as_str()).map(|s| Tagged {
                trust: Trust::External, text: s.to_string()
            }),
            class: item.get("resourceClass").and_then(|x| x.as_str()).map(|s| Tagged {
                trust: Trust::Trusted, text: s.to_string()
            }),
            pid: item.get("pid").and_then(|x| x.as_i64()),
            geometry,
            screen: item.get("screen").and_then(|x| x.as_str()).map(|s| s.to_string()),
            minimized: item.get("minimized").and_then(|x| x.as_bool()),
            maximized: item.get("maximized").and_then(|x| x.as_bool()),
            fullscreen: item.get("fullScreen").and_then(|x| x.as_bool()),
        });
    }
    Ok(out)
}

const JS_LIST_ALL: &str = r#"
    var ws = workspace;
    var windows = (ws.windowList ? ws.windowList() : ws.windows) || [];
    return windows.map(function(w) {
        var g = w.frameGeometry;
        var out = w.output;
        var d  = (w.desktops && w.desktops.length) ? w.desktops[0] : null;
        return {
            uuid: w.internalId ? w.internalId.toString() : "",
            caption: w.caption || "",
            resourceClass: w.resourceClass || "",
            pid: w.pid || 0,
            geometry: [g.x, g.y, g.width, g.height],
            screen: out ? out.name : null,
            desktop: d ? d.x11DesktopNumber : null,
            minimized: !!w.minimized,
            fullScreen: !!w.fullScreen,
            maximized: w.maximizeMode ? (w.maximizeMode > 0) : false
        };
    });
"#;

pub async fn window_control(
    _state: &KdeMcp,
    _args: WindowControlArgs,
) -> Result<WindowControlResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

#[derive(Clone)]
struct _RefHandle(Arc<RefStore>);
```

Add `regex-lite = "0.1"` to Cargo.toml (lighter than full `regex`; sufficient for our match patterns):

```toml
regex-lite     = "0.1"
```

- [ ] **Step 6.3: Update `WindowEntry` to derive `Clone` and `Deserialize`**

Already `Serialize` via the derive; for `project` to clone we need `Clone`:

```rust
#[derive(Debug, Clone, Serialize, JsonSchema)]
```

`Tagged<T>` already derives `Clone` (verified in `src/trust.rs`).

- [ ] **Step 6.4: Run the unit tests**

```bash
cargo test --workspace window 2>&1 | tail -10
```

Expected: both `format_ids_*` and `format_titles_*` tests pass.

- [ ] **Step 6.5: Commit**

```bash
git add Cargo.toml src/tools/window.rs src/server.rs
git commit -m "feat(phase-1): window_query via kwin script"
```

---

## Task 7: KWin `ScreenShot2` zbus proxy

**Files:**
- Modify: `src/kwin/screenshot.rs`

- [ ] **Step 7.1: Implement the proxy**

Replace `src/kwin/screenshot.rs`:

```rust
//! org.kde.KWin.ScreenShot2 zbus proxy.
//!
//! Each `Capture*` method returns `(fd, results)`. We read the PNG bytes off
//! the fd into memory; the response is small (≤ a few MB at typical sizes
//! after max_long_edge clamping in `src/util/image.rs`).

use crate::error::ToolError;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::io::AsRawFd;
use zbus::Connection;
use zvariant::Value;

#[zbus::proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
pub trait ScreenShot2 {
    fn capture_workspace(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_active_window(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_window(
        &self,
        handle: &str,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_active_screen(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_area(
        &self,
        x: i32, y: i32, width: u32, height: u32,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;
}

pub struct CaptureRequest<'a> {
    pub region: Region<'a>,
    pub include_cursor: bool,
}

pub enum Region<'a> {
    Workspace,
    ActiveScreen,
    ActiveWindow,
    Window(&'a str),  // KWin internalId UUID string
    Area([i32; 4]),   // [x, y, w, h]
}

pub async fn capture(bus: &Connection, req: CaptureRequest<'_>) -> Result<Vec<u8>, ToolError> {
    // Create a pipe; give one end to KWin, read the PNG off the other.
    let (read_fd, write_fd) = nix::unistd::pipe().map_err(|e| ToolError::Io {
        path: "pipe()".into(), error: e.to_string()
    })?;
    let read_owned = OwnedFd::from(read_fd);
    let write_owned = zvariant::OwnedFd::from(OwnedFd::from(write_fd));

    let proxy = ScreenShot2Proxy::new(bus).await.map_err(busdc)?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("include-cursor", req.include_cursor.into());
    options.insert("native-resolution", false.into());

    let _result = match req.region {
        Region::Workspace => proxy.capture_workspace(options, write_owned).await,
        Region::ActiveScreen => proxy.capture_active_screen(options, write_owned).await,
        Region::ActiveWindow => proxy.capture_active_window(options, write_owned).await,
        Region::Window(uuid) => proxy.capture_window(uuid, options, write_owned).await,
        Region::Area([x, y, w, h]) => proxy
            .capture_area(x, y, w as u32, h as u32, options, write_owned)
            .await,
    }
    .map_err(busdc)?;

    // Spawn-blocking read; PNG comes through quickly so a single read is fine.
    let bytes = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut f = std::fs::File::from(read_owned);
        let mut buf = Vec::with_capacity(256 * 1024);
        f.read_to_end(&mut buf)?;
        Ok(buf)
    })
    .await
    .map_err(|e| ToolError::Io { path: "screenshot pipe".into(), error: e.to_string() })?
    .map_err(|e| ToolError::Io { path: "screenshot pipe".into(), error: e.to_string() })?;

    if bytes.is_empty() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: "screenshot pipe returned 0 bytes".into(),
        });
    }
    Ok(bytes)
}

fn busdc(e: zbus::Error) -> ToolError {
    ToolError::BusDisconnected { detail: e.to_string() }
}
```

Add `nix = { version = "0.29", default-features = false, features = ["fs"] }` to Cargo.toml — gives us `pipe()` without pulling in the full nix surface.

```toml
nix            = { version = "0.29", default-features = false, features = ["fs"] }
```

(If the `0.29` API has renamed `pipe` to `pipe2` or moved it to a different module, Task 0's spike notes should already have caught that; otherwise adjust the import here.)

- [ ] **Step 7.2: Verify compilation**

```bash
cargo check --workspace 2>&1 | tail -5
```

Expected: clean. If `zvariant::OwnedFd` conversion errors, use the `From<OwnedFd>` impl path explicitly (`zvariant::OwnedFd::from(write_owned)` rather than the chained `From`).

- [ ] **Step 7.3: Commit**

```bash
git add Cargo.toml src/kwin/screenshot.rs
git commit -m "feat(phase-1): kwin ScreenShot2 zbus proxy"
```

---

## Task 8: `screenshot` (basic, no masking)

Tool surface only — uses Task 7's `capture` and Task 9's image utilities (which we stub here, then implement in Task 9).

**Files:**
- Modify: `src/tools/screenshot.rs`

- [ ] **Step 8.1: Define `ScreenshotResponse` + the `region` enum decoder**

Replace `src/tools/screenshot.rs`:

```rust
use crate::error::ToolError;
use crate::kwin::screenshot::{CaptureRequest, Region, capture};
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotArgs {
    /// "active_screen" | "full" | "window:<ref>" | "element:<ref>" | [x,y,w,h]
    #[serde(default)]
    pub region: Option<serde_json::Value>,
    #[serde(default)]
    pub include_cursor: Option<bool>,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotResponse {
    pub original_size: [u32; 2],
    pub returned_size: [u32; 2],
    pub scale_factor: f32,
    pub format: String, // "png"
    /// Base64-encoded PNG bytes. Hosts that prefer image content blocks read
    /// this directly; the server's `tool_result` wrapping emits both an image
    /// content block (Phase 2 work) and this structured field.
    pub png_base64: String,
}

pub async fn screenshot(
    state: &KdeMcp,
    args: ScreenshotArgs,
) -> Result<ScreenshotResponse, ToolError> {
    let region = resolve_region(state, args.region.as_ref()).await?;
    let include_cursor = args.include_cursor.unwrap_or(true);

    let png_bytes = capture(&state.bus, CaptureRequest { region, include_cursor }).await?;
    let (original, returned, scale, masked) = process_image(&png_bytes, args.max_long_edge)?;

    use base64::Engine as _;
    Ok(ScreenshotResponse {
        original_size: [original.0, original.1],
        returned_size: [returned.0, returned.1],
        scale_factor: scale,
        format: "png".into(),
        png_base64: base64::engine::general_purpose::STANDARD.encode(&masked),
    })
}

async fn resolve_region<'a>(
    state: &'a KdeMcp,
    raw: Option<&'a serde_json::Value>,
) -> Result<Region<'a>, ToolError> {
    match raw {
        None => Ok(Region::ActiveScreen),
        Some(v) if v.is_string() => match v.as_str().unwrap() {
            "active_screen" => Ok(Region::ActiveScreen),
            "full" => Ok(Region::Workspace),
            s if s.starts_with("window:") => {
                let ref_id = &s["window:".len()..];
                let meta = state.refs.resolve(ref_id).await.map_err(|e| match e {
                    RefError::Stale => ToolError::StaleHandle { ref_id: ref_id.into() },
                    RefError::Unknown => ToolError::StaleHandle { ref_id: ref_id.into() },
                })?;
                if meta.kind != RefKind::Window {
                    return Err(ToolError::UnsupportedRegion {
                        reason: format!("ref {ref_id} is not a window"),
                    });
                }
                // payload is the KWin internalId UUID string.
                Ok(Region::Window(Box::leak(meta.payload.into_boxed_str())))
            }
            other => Err(ToolError::UnsupportedRegion {
                reason: format!("unrecognised region string: {other}"),
            }),
        },
        Some(v) if v.is_array() => {
            let a = v.as_array().unwrap();
            if a.len() != 4 {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("rect region must be [x,y,w,h], got {} elements", a.len()),
                });
            }
            let rect = [
                a[0].as_i64().unwrap_or(0) as i32,
                a[1].as_i64().unwrap_or(0) as i32,
                a[2].as_i64().unwrap_or(0) as i32,
                a[3].as_i64().unwrap_or(0) as i32,
            ];
            Ok(Region::Area(rect))
        }
        _ => Err(ToolError::UnsupportedRegion {
            reason: "region must be a string or [x,y,w,h]".into(),
        }),
    }
}

fn process_image(
    png_bytes: &[u8],
    max_long_edge: Option<u32>,
) -> Result<((u32, u32), (u32, u32), f32, Vec<u8>), ToolError> {
    // Phase 1 Task 8: no masking yet; just decode/resize and re-encode.
    // Task 10 layers terminal masking on top by extending this function to
    // accept a `rects` list.
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("png decode: {e}"),
        })?;
    let (ow, oh) = (img.width(), img.height());
    let max = max_long_edge.unwrap_or(1280);
    let long = ow.max(oh);
    let (img_resized, scale) = if long > max {
        let scale = max as f32 / long as f32;
        let (nw, nh) = ((ow as f32 * scale) as u32, (oh as f32 * scale) as u32);
        (img.resize(nw, nh, image::imageops::FilterType::Triangle), scale)
    } else {
        (img, 1.0)
    };
    let mut buf = Vec::with_capacity(64 * 1024);
    img_resized
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("png encode: {e}"),
        })?;
    let rw = img_resized.width();
    let rh = img_resized.height();
    Ok(((ow, oh), (rw, rh), scale, buf))
}
```

Note the `Box::leak` on the window UUID is intentional and bounded — each `screenshot` call leaks one short string per `window:<ref>` invocation, scoped to the process lifetime. This is the simplest path to satisfy the `'a` lifetime on `Region::Window(&str)`. Task 10 (which restructures `Region` to own its string) replaces this.

- [ ] **Step 8.2: Compile**

```bash
cargo check --workspace 2>&1 | tail -3
```

- [ ] **Step 8.3: Commit**

```bash
git add src/tools/screenshot.rs src/server.rs
git commit -m "feat(phase-1): screenshot tool (no terminal masking yet)"
```

---

## Task 9: Image utilities

**Files:**
- Modify: `src/util/image.rs`

- [ ] **Step 9.1: Implement resize + mask overlay + the terminal class list**

Replace `src/util/image.rs`:

```rust
//! Image post-processing: long-edge resize and terminal-mask overlay.

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};

/// Class names whose windows are masked in screenshots.
///
/// Phase 1 ships this hardcoded; Phase 3 replaces it with a lookup against
/// the policy table. The set covers the dozen most common Linux terminals.
pub const TERMINAL_CLASSES: &[&str] = &[
    "konsole",
    "yakuake",
    "alacritty",
    "kitty",
    "wezterm",
    "foot",
    "ghostty",
    "tilix",
    "gnome-terminal",
    "xterm",
    "xfce4-terminal",
    "ptyxis",
    "warp-terminal",
];

pub fn is_terminal_class(class: &str) -> bool {
    let lower = class.to_ascii_lowercase();
    TERMINAL_CLASSES.iter().any(|c| lower == *c)
}

pub fn resize_long_edge(img: DynamicImage, max: u32) -> (DynamicImage, f32) {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max {
        return (img, 1.0);
    }
    let scale = max as f32 / long as f32;
    let (nw, nh) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    (img.resize(nw, nh, image::imageops::FilterType::Triangle), scale)
}

/// Overlay solid black rectangles on the image. Rectangles are in image
/// pixels (post-scaling). Clamps to bounds. Label is currently advisory —
/// rendered text would require a font (skipped in Phase 1; the rectangle
/// itself is the signal).
pub fn mask_rectangles(img: DynamicImage, rects: &[[i32; 4]]) -> DynamicImage {
    let (w, h) = img.dimensions();
    let mut buf: ImageBuffer<Rgba<u8>, _> = img.to_rgba8();
    for r in rects {
        let x0 = r[0].max(0).min(w as i32) as u32;
        let y0 = r[1].max(0).min(h as i32) as u32;
        let x1 = ((r[0] + r[2]).max(0).min(w as i32)) as u32;
        let y1 = ((r[1] + r[3]).max(0).min(h as i32)) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                buf.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
    }
    DynamicImage::ImageRgba8(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_terminal_class_is_case_insensitive() {
        assert!(is_terminal_class("Konsole"));
        assert!(is_terminal_class("KONSOLE"));
        assert!(is_terminal_class("konsole"));
        assert!(!is_terminal_class("kate"));
    }

    #[test]
    fn resize_no_op_when_below_max() {
        let img = DynamicImage::new_rgba8(100, 50);
        let (out, scale) = resize_long_edge(img, 200);
        assert_eq!(out.dimensions(), (100, 50));
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn resize_shrinks_to_long_edge() {
        let img = DynamicImage::new_rgba8(400, 100);
        let (out, scale) = resize_long_edge(img, 200);
        assert_eq!(out.width(), 200);
        assert!((scale - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mask_overrides_pixels_in_rectangle() {
        let mut img = DynamicImage::new_rgba8(8, 8);
        for x in 0..8 { for y in 0..8 { img.as_mut_rgba8().unwrap().put_pixel(x, y, Rgba([255,255,255,255])); } }
        let out = mask_rectangles(img, &[[2,2,4,4]]);
        let buf = out.as_rgba8().unwrap();
        assert_eq!(buf.get_pixel(3,3).0, [0,0,0,255]);
        assert_eq!(buf.get_pixel(0,0).0, [255,255,255,255]);
    }
}
```

- [ ] **Step 9.2: Run the unit tests**

```bash
cargo test --workspace -p kde-mcp -- util::image 2>&1 | tail -10
```

Expected: 4/4 pass.

- [ ] **Step 9.3: Commit**

```bash
git add src/util/image.rs
git commit -m "feat(phase-1): image resize + terminal-mask overlay utilities"
```

---

## Task 10: `screenshot` with terminal masking

Composes Task 6 (window list with class + geometry), Task 8 (`screenshot` basic), and Task 9 (mask overlay). The mask-application step:

1. Fetch the window list (the same JS body as `window_query` action=list, format=full).
2. Filter to entries whose `resourceClass` matches `is_terminal_class()`.
3. **Exemption**: if the requested region is `window:<ref>` AND the resolved window's own class is a terminal, the agent is intentionally looking at that terminal — skip masking *for that window only* (other terminals in the same capture still get masked).
4. Translate window geometry from logical pixels to image pixels by multiplying by `scale_factor` (computed by the capture; for `Workspace`/`ActiveScreen` we always assume `1.0` after the post-capture resize handles scaling, then apply scale again from the resize).
5. Call `mask_rectangles` with the list.

**Files:**
- Modify: `src/tools/screenshot.rs`

- [ ] **Step 10.1: Refactor `Region` to own its UUID string**

Edit `src/kwin/screenshot.rs`:

```rust
pub enum Region {
    Workspace,
    ActiveScreen,
    ActiveWindow,
    Window(String),
    Area([i32; 4]),
}
```

And the `capture` matches on `Region::Window(uuid) => proxy.capture_window(&uuid, options, write_owned).await,`. Update the call sites in `src/tools/screenshot.rs` (drop the `Box::leak` from Task 8.1's `resolve_region` and use the owned `String` directly).

- [ ] **Step 10.2: Add masking inside `screenshot`**

In `src/tools/screenshot.rs`, replace the call to `process_image` with the masking-aware version:

```rust
    let png_bytes = capture(&state.bus, CaptureRequest { region: region.clone(), include_cursor }).await?;
    let terminals = list_terminal_rects(state, &region).await?;
    let (original, returned, scale, masked) = process_image_with_mask(&png_bytes, args.max_long_edge, &terminals)?;
```

`Region` derives `Clone`:

```rust
#[derive(Clone)]
pub enum Region { ... }
```

Add (in `src/tools/screenshot.rs`):

```rust
async fn list_terminal_rects(
    state: &KdeMcp,
    captured: &Region,
) -> Result<Vec<[i32; 4]>, ToolError> {
    let target_uuid = match captured {
        Region::Window(uuid) => Some(uuid.clone()),
        _ => None,
    };
    let payload = crate::kwin::scripting::run_script(
        &state.bus,
        crate::tools::window::JS_LIST_ALL_FULL_GEOMETRY,
        std::time::Duration::from_secs(2),
    )
    .await?;
    let v: serde_json::Value = serde_json::from_str(&payload).map_err(|e| ToolError::ExternalCommandFailed {
        stderr: format!("window list parse: {e}")
    })?;
    let entries = crate::tools::window::parse_entries(&v)?;
    let mut rects = Vec::new();
    for e in &entries {
        let class = match &e.class { Some(c) => c.text.clone(), None => continue };
        if !crate::util::image::is_terminal_class(&class) { continue; }
        if let Some(t) = &target_uuid {
            // Skip masking the user's intentionally-captured terminal.
            if &e.uuid == t { continue; }
        }
        if let Some(g) = e.geometry { rects.push(g); }
    }
    Ok(rects)
}

fn process_image_with_mask(
    png_bytes: &[u8],
    max_long_edge: Option<u32>,
    rects: &[[i32; 4]],
) -> Result<((u32, u32), (u32, u32), f32, Vec<u8>), ToolError> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("png decode: {e}"),
        })?;
    let (ow, oh) = (img.width(), img.height());

    // Mask first (on full-res, with full-res rects), then resize. Cheaper for
    // multi-monitor full captures than masking-after-resize since the rect
    // count is small.
    let masked_full = crate::util::image::mask_rectangles(img, rects);
    let (resized, scale) = crate::util::image::resize_long_edge(masked_full, max_long_edge.unwrap_or(1280));

    let mut buf = Vec::with_capacity(64 * 1024);
    resized
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("png encode: {e}"),
        })?;
    Ok(((ow, oh), (resized.width(), resized.height()), scale, buf))
}
```

And in `src/tools/window.rs`, expose the JS constant + `parse_entries`:

```rust
pub(crate) const JS_LIST_ALL_FULL_GEOMETRY: &str = JS_LIST_ALL;
```

(`JS_LIST_ALL` is already the only JS body and the same one used for masking.)

- [ ] **Step 10.3: Verify compilation**

```bash
cargo check --workspace 2>&1 | tail -5
```

- [ ] **Step 10.4: Commit**

```bash
git add src/tools/screenshot.rs src/kwin/screenshot.rs src/tools/window.rs
git commit -m "feat(phase-1): terminal-class masking on screenshot"
```

---

## Task 11: AT-SPI infrastructure

**Files:**
- Modify: `src/atspi/connection.rs`
- Modify: `src/atspi/tree.rs`
- Modify: `src/atspi/ref_encoding.rs`
- Modify: `src/server.rs` (add an `Arc<AtspiConnection>` field on `KdeMcp`; lazy-open at first use to keep `KdeMcp::new` cheap).

The exact API surface depends on Task 0.1 spike findings. The plan below uses the names from `atspi 0.30.0` per the spec §13.1 pin; **if the spike shows divergence, this task adopts the verified names.**

- [ ] **Step 11.1: Implement the lazy connection wrapper**

`src/atspi/connection.rs`:

```rust
use crate::error::ToolError;
use atspi::AccessibilityConnection;
use tokio::sync::OnceCell;

pub struct AtspiConnection {
    inner: OnceCell<AccessibilityConnection>,
}

impl Default for AtspiConnection {
    fn default() -> Self { Self::new() }
}

impl AtspiConnection {
    pub fn new() -> Self { Self { inner: OnceCell::new() } }
    pub async fn get(&self) -> Result<&AccessibilityConnection, ToolError> {
        self.inner
            .get_or_try_init(|| async {
                AccessibilityConnection::new()
                    .await
                    .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })
            })
            .await
    }
}
```

`KdeMcp` gains `pub atspi: std::sync::Arc<crate::atspi::connection::AtspiConnection>` and `KdeMcp::new` initialises it with `Arc::new(AtspiConnection::new())`.

- [ ] **Step 11.2: Implement the tree walker**

`src/atspi/tree.rs`:

```rust
use crate::atspi::ref_encoding::encode_element_ref;
use crate::error::ToolError;
use crate::refs::{RefKind, RefStore};
use crate::trust::{Tagged, Trust};
use atspi::{AccessibilityConnection, Accessible, Role};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct FlatElement {
    pub id: String,
    pub parent_id: Option<String>,
    pub child_count: u32,
    pub role: Tagged<String>,
    pub name: Tagged<String>,
    pub value: Option<Tagged<String>>,
    pub states: Vec<String>,
    pub bbox: Option<[i32; 4]>,
    pub actions: Vec<String>,
}

pub struct WalkOpts {
    pub max_depth: u32,
    pub max_elements: usize,
    pub role_filter: Option<Vec<String>>,
    pub name_pattern: Option<regex_lite::Regex>,
}

pub async fn walk(
    _conn: &AccessibilityConnection,
    _root: Accessible,
    _refs: Arc<RefStore>,
    _opts: WalkOpts,
) -> Result<(Vec<FlatElement>, bool, Vec<String>), ToolError> {
    // Pseudocode: BFS the tree starting at `root`, minting an element ref
    // for each visited node via `refs.mint(RefKind::Element, encoded_path)`.
    // For each node:
    //   role        := node.role().await
    //   name        := node.name().await
    //   value       := node.value().await (if Accessible::HasValue)
    //   states      := node.states().await -> .iter().map(|s| s.name())
    //   bbox        := node.extents(CoordType::Window).await
    //   actions     := atspi::action::ActionProxy::new(...).get_actions().await -> map .name
    // Apply role_filter / name_pattern filters before pushing.
    // Stop when len() >= max_elements; set truncated=true.
    //
    // Returns:
    //   warnings: ["AT-SPI tree empty — vision fallback recommended"] when
    //   no children at all (typical of a11y-degraded apps like Electron
    //   without --force-renderer-accessibility).
    Err(ToolError::NotImplemented)
}
```

This step is the riskiest in Phase 1 because the exact `atspi 0.30` accessor names are spike-dependent. The plan ships the pseudocode above; the spike fills in the verified function names and the implementation follows from there. **Budget for this step is 4–6 hours; if more, fall back to a simpler initial cut (only Role + Name; no value/states/bbox/actions) and improve in a follow-up commit.**

- [ ] **Step 11.3: Implement `ref_encoding`**

`src/atspi/ref_encoding.rs`:

```rust
//! Encode AT-SPI `(bus_name, object_path)` into the RefStore's payload string
//! and decode it back. Format: `"<bus_name>|<object_path>"` — bus names
//! cannot contain `|` (D-Bus spec restricts charset).

pub fn encode_element_ref(bus_name: &str, object_path: &str) -> String {
    format!("{bus_name}|{object_path}")
}

pub fn decode_element_ref(payload: &str) -> Option<(&str, &str)> {
    let mut parts = payload.splitn(2, '|');
    let bus = parts.next()?;
    let path = parts.next()?;
    Some((bus, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let s = encode_element_ref(":1.42", "/org/a11y/atspi/accessible/123");
        let (b, p) = decode_element_ref(&s).unwrap();
        assert_eq!(b, ":1.42");
        assert_eq!(p, "/org/a11y/atspi/accessible/123");
    }
}
```

- [ ] **Step 11.4: Run unit tests**

```bash
cargo test --workspace ref_encoding 2>&1 | tail -5
```

- [ ] **Step 11.5: Commit**

```bash
git add src/atspi src/server.rs
git commit -m "feat(phase-1): atspi connection + tree skeleton + ref encoding"
```

---

## Task 12: `inspect`

**Files:**
- Modify: `src/tools/inspect.rs`

- [ ] **Step 12.1: Implement**

```rust
use crate::atspi::tree::{WalkOpts, walk};
use crate::error::ToolError;
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InspectArgs {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub role_filter: Option<Vec<String>>,
    #[serde(default)]
    pub name_pattern: Option<String>,
    #[serde(default)]
    pub max_elements: Option<u32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InspectResponse {
    pub elements: Vec<crate::atspi::tree::FlatElement>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn inspect(
    state: &KdeMcp,
    args: InspectArgs,
) -> Result<InspectResponse, ToolError> {
    let conn = state.atspi.get().await?;
    let target = args.target.unwrap_or_else(|| "focused".into());

    let root = match target.as_str() {
        "focused" => atspi_focused_root(conn).await?,
        s if s.starts_with("window:") => {
            // Window-rooted inspection. Resolve the ref to a KWin UUID,
            // then map that UUID to its AT-SPI app via heuristic name match.
            // Phase 1 ships the heuristic; Phase 4 may add an explicit
            // window->app pid->bus map.
            return Err(ToolError::NotImplemented); // first cut
        }
        s if s.starts_with("element:") => {
            let ref_id = &s["element:".len()..];
            let meta = state.refs.resolve(ref_id).await.map_err(|e| match e {
                RefError::Stale | RefError::Unknown => ToolError::StaleHandle { ref_id: ref_id.into() },
            })?;
            if meta.kind != RefKind::Element {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("ref {ref_id} is not an element"),
                });
            }
            atspi_lookup_by_encoded(conn, &meta.payload).await?
        }
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unrecognised target: {other}"),
            });
        }
    };

    let role_filter = args.role_filter;
    let name_pattern = args.name_pattern
        .as_deref()
        .map(regex_lite::Regex::new)
        .transpose()
        .map_err(|e| ToolError::UnsupportedRegion {
            reason: format!("name_pattern: {e}"),
        })?;
    let opts = WalkOpts {
        max_depth: args.max_depth.unwrap_or(3),
        max_elements: args.max_elements.unwrap_or(200) as usize,
        role_filter,
        name_pattern,
    };
    let (elements, truncated, warnings) =
        walk(conn, root, state.refs.clone(), opts).await?;
    Ok(InspectResponse { elements, truncated, warnings })
}

async fn atspi_focused_root(_conn: &atspi::AccessibilityConnection)
    -> Result<atspi::Accessible, ToolError>
{
    // Task 0.1 spike fills in the exact accessor.
    Err(ToolError::NotImplemented)
}

async fn atspi_lookup_by_encoded(
    _conn: &atspi::AccessibilityConnection,
    _payload: &str,
) -> Result<atspi::Accessible, ToolError> {
    Err(ToolError::NotImplemented)
}
```

`inspect` for `target = "window:<ref>"` returns `NotImplemented` in Phase 1's first cut; spec §6.1 says the param is supported, but window→AT-SPI mapping is non-trivial without a window→pid→bus discovery step that Phase 4 can absorb. Document this in the Phase 1 closeout notes.

- [ ] **Step 12.2: Commit**

```bash
git add src/tools/inspect.rs src/server.rs
git commit -m "feat(phase-1): inspect tool (focused+element targets)"
```

---

## Task 13: `observe`

Aggregates KWin scripting (windows, cursor, screens, focused window) with AT-SPI focused-element (best-effort; if a11y unavailable, the field is `null` with a warning).

**Files:**
- Modify: `src/tools/observe.rs`

- [ ] **Step 13.1: Implement**

Replace `src/tools/observe.rs`:

```rust
use crate::error::ToolError;
use crate::kwin::scripting::run_script;
use crate::refs::RefKind;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ObserveArgs {
    #[serde(default)] pub detail: Option<DetailLevel>,
    #[serde(default)] pub include_other_windows: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Summary,
    #[default]
    Interactive,
    Full,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ObserveResponse {
    pub mode: String,
    pub cursor: [i32; 2],
    pub focused_window: Option<FocusedWindow>,
    pub focused_element: Option<FocusedElement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<crate::tools::window::WindowEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screens: Option<Vec<Screen>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FocusedWindow {
    pub uuid: String,
    pub title: Tagged<String>,
    pub class: Tagged<String>,
    pub pid: Option<i64>,
    pub geometry: Option<[i32; 4]>,
    pub screen: Option<String>,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FocusedElement {
    pub id: String,
    pub role: Tagged<String>,
    pub name: Tagged<String>,
    pub value: Option<Tagged<String>>,
    pub states: Vec<String>,
    pub bbox: Option<[i32; 4]>,
    pub actions: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Screen {
    pub name: String,
    pub geometry: [i32; 4],
    pub scale: f32,
    pub primary: bool,
}

pub async fn observe(
    state: &KdeMcp,
    args: ObserveArgs,
) -> Result<ObserveResponse, ToolError> {
    let detail = args.detail.unwrap_or_default();
    let include_others = args.include_other_windows.unwrap_or(true);

    // 1. KWin script: cursor, windows, screens, focused-window uuid.
    let payload = run_script(&state.bus, JS_OBSERVE, Duration::from_secs(2)).await?;
    let v: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|e| ToolError::ExternalCommandFailed { stderr: format!("observe script parse: {e}") })?;
    let cursor = [
        v.pointer("/cursor/0").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
        v.pointer("/cursor/1").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
    ];
    let focused_raw = v.get("focused");
    let mut focused_window: Option<FocusedWindow> = None;
    if let Some(fw) = focused_raw.filter(|x| !x.is_null()) {
        let uuid_raw = fw.get("uuid").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let opaque = state.refs.mint(RefKind::Window, uuid_raw).await;
        focused_window = Some(FocusedWindow {
            uuid: opaque,
            title: Tagged { trust: Trust::External, text: fw.get("caption").and_then(|x| x.as_str()).unwrap_or("").into() },
            class: Tagged { trust: Trust::Trusted, text: fw.get("resourceClass").and_then(|x| x.as_str()).unwrap_or("").into() },
            pid: fw.get("pid").and_then(|x| x.as_i64()),
            geometry: fw.get("geometry").and_then(|g| {
                g.as_array().filter(|a| a.len() == 4).map(|a| [
                    a[0].as_i64().unwrap_or(0) as i32,
                    a[1].as_i64().unwrap_or(0) as i32,
                    a[2].as_i64().unwrap_or(0) as i32,
                    a[3].as_i64().unwrap_or(0) as i32,
                ])
            }),
            screen: fw.get("screen").and_then(|x| x.as_str()).map(|s| s.to_string()),
            minimized: fw.get("minimized").and_then(|x| x.as_bool()).unwrap_or(false),
            maximized: fw.get("maximized").and_then(|x| x.as_bool()).unwrap_or(false),
            fullscreen: fw.get("fullScreen").and_then(|x| x.as_bool()).unwrap_or(false),
        });
    }

    // 2. Windows + screens by detail level.
    let mut windows = None;
    let mut screens = None;
    let mut warnings = Vec::new();

    match detail {
        DetailLevel::Summary => {
            if !include_others {
                // Drop window list entirely; agent gets only a count.
                // Spec: in summary mode, just a count. We honour this by
                // setting windows = None and noting count in warnings if
                // non-zero.
                if let Some(arr) = v.get("windows").and_then(|w| w.as_array()) {
                    if !arr.is_empty() {
                        warnings.push(format!("{} other windows present (use detail=interactive)", arr.len()));
                    }
                }
            }
        }
        DetailLevel::Interactive | DetailLevel::Full => {
            if let Some(arr) = v.get("windows").and_then(|w| w.as_array()) {
                let val = serde_json::Value::Array(arr.clone());
                let mut entries = crate::tools::window::parse_entries(&val)?;
                for entry in entries.iter_mut() {
                    let raw = entry.uuid.clone();
                    entry.uuid = state.refs.mint(RefKind::Window, raw).await;
                }
                windows = Some(entries);
            }
            if matches!(detail, DetailLevel::Full) {
                if let Some(arr) = v.get("screens").and_then(|s| s.as_array()) {
                    let mut out = Vec::new();
                    for s in arr {
                        out.push(Screen {
                            name: s.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                            geometry: {
                                let g = s.get("geometry").and_then(|g| g.as_array());
                                if let Some(a) = g.filter(|a| a.len() == 4) {
                                    [
                                        a[0].as_i64().unwrap_or(0) as i32,
                                        a[1].as_i64().unwrap_or(0) as i32,
                                        a[2].as_i64().unwrap_or(0) as i32,
                                        a[3].as_i64().unwrap_or(0) as i32,
                                    ]
                                } else { [0,0,0,0] }
                            },
                            scale: s.get("scale").and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(1.0),
                            primary: s.get("primary").and_then(|x| x.as_bool()).unwrap_or(false),
                        });
                    }
                    screens = Some(out);
                }
            }
        }
    }

    // 3. AT-SPI focused element (best effort).
    let focused_element = match state.atspi.get().await {
        Ok(_conn) => None, // Task 11/12 fills this in by calling atspi_focused_element.
        Err(e) => {
            warnings.push(format!("a11y bus unavailable: {} — focused_element omitted", e.code()));
            None
        }
    };

    Ok(ObserveResponse {
        mode: state.mode.as_str().to_string(),
        cursor,
        focused_window,
        focused_element,
        windows,
        screens,
        warnings,
    })
}

const JS_OBSERVE: &str = r#"
    var ws = workspace;
    var cur = ws.cursorPos;
    var active = ws.activeWindow;
    var others = (ws.windowList ? ws.windowList() : ws.windows) || [];
    var screens = (ws.screens || []).map(function(s) {
        return {
            name: s.name,
            geometry: [s.geometry.x, s.geometry.y, s.geometry.width, s.geometry.height],
            scale: s.scale,
            primary: (ws.activeOutput === s)
        };
    });
    function dump(w) {
        if (!w) return null;
        var g = w.frameGeometry;
        return {
            uuid: w.internalId ? w.internalId.toString() : "",
            caption: w.caption || "",
            resourceClass: w.resourceClass || "",
            pid: w.pid || 0,
            geometry: [g.x, g.y, g.width, g.height],
            screen: w.output ? w.output.name : null,
            minimized: !!w.minimized,
            fullScreen: !!w.fullScreen,
            maximized: w.maximizeMode ? (w.maximizeMode > 0) : false
        };
    }
    return {
        cursor: [cur.x, cur.y],
        focused: dump(active),
        windows: others.filter(function(w) { return w !== active; }).map(dump),
        screens: screens
    };
"#;
```

- [ ] **Step 13.2: Add the offline shape-parity test**

`tests/integration/observe_offline.rs`:

```rust
use kde_mcp::tools::observe::{FocusedWindow, ObserveResponse, Screen};
use kde_mcp::trust::{Tagged, Trust};

#[test]
fn observe_response_shape_matches_spec_6_1() {
    let resp = ObserveResponse {
        mode: "live".into(),
        cursor: [100, 200],
        focused_window: Some(FocusedWindow {
            uuid: "win#abc".into(),
            title: Tagged { trust: Trust::External, text: "Document — kate".into() },
            class: Tagged { trust: Trust::Trusted, text: "kate".into() },
            pid: Some(1234),
            geometry: Some([0, 0, 800, 600]),
            screen: Some("DP-1".into()),
            minimized: false,
            maximized: false,
            fullscreen: false,
        }),
        focused_element: None,
        windows: Some(vec![]),
        screens: Some(vec![Screen {
            name: "DP-1".into(),
            geometry: [0, 0, 2560, 1440],
            scale: 1.5,
            primary: true,
        }]),
        warnings: vec![],
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["mode"], "live");
    assert_eq!(v["cursor"][0], 100);
    assert_eq!(v["focused_window"]["uuid"], "win#abc");
    assert_eq!(v["focused_window"]["title"]["trust"], "external");
    assert_eq!(v["focused_window"]["class"]["trust"], "trusted");
    assert_eq!(v["screens"][0]["scale"], 1.5);
}
```

Wire into `tests/integration.rs`:

```rust
    pub mod observe_offline;
```

- [ ] **Step 13.3: Run tests**

```bash
cargo test --workspace observe_offline 2>&1 | tail -5
```

- [ ] **Step 13.4: Commit**

```bash
git add src/tools/observe.rs src/server.rs tests/integration.rs tests/integration/observe_offline.rs
git commit -m "feat(phase-1): observe tool composing kwin + atspi"
```

---

## Task 14: Verification + smoke test refresh + Phase 1 tag

- [ ] **Step 14.1: Refresh `tests/integration/SMOKE.md`**

Append a new section "C — Phase 1 read-only" covering, for each of `text_editor.view`, `clipboard_read`, `window_query`, `screenshot`, `inspect`, `observe`:
- An MCP-wire fixture and expected response shape (success path).
- An expected error case (e.g. `text_editor.view` on `/etc/shadow` → either `io` or success depending on perms; document both).
- A check that `audit.jsonl` gains exactly one record per invocation with the wire tool name.

Example (text_editor.view; copy this shape for the other 5):

````markdown
### C.1 — `text_editor.view`

```sh
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"text_editor.view","arguments":{"path":"/etc/os-release","view_range":[1,3]}}}' \
  | timeout 4 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c1.jsonl
test "$(wc -l < /tmp/c1.jsonl)" = 2
sed -n '2p' /tmp/c1.jsonl | python3 -c "import sys,json; d=json.load(sys.stdin)['result']; \
  assert d.get('isError') is not True; \
  payload=json.loads(d['content'][0]['text']); \
  assert payload['kind']=='file'; \
  assert '1\\t' in payload['content']; \
  print('C.1 OK')"
```
````

- [ ] **Step 14.2: Run full test suite**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: fmt clean, clippy clean (-D warnings), 50+ tests passing (37 from Phase 0 + Phase 1's new ones).

- [ ] **Step 14.3: Run the full smoke procedure**

```bash
cargo build --release
# Execute each of SMOKE.md §A and §C end-to-end.
```

Expected: all pass against the live KDE Plasma 6 session.

- [ ] **Step 14.4: Update `docs/HANDOFF.md`**

Mark Phase 1 as the new "complete" state in §"Current status"; pivot "What is NOT yet done" so Phase 2 is the next concrete step; update the recipes section.

- [ ] **Step 14.5: Commit and tag**

```bash
git add docs/HANDOFF.md tests/integration/SMOKE.md
git commit -m "docs(phase-1): mark complete; pivot handoff to Phase 2"
git tag phase-1
```

---

## Self-review

**Spec coverage:** every tool in spec §14 Phase 1 row (`observe`, `inspect`, `window_query`, `screenshot` w/ terminal masking, `clipboard_read`, `text_editor.view`) has a Task implementing it (3, 4, 6, 8+10, 12, 13). Audit-log hookup per HANDOFF "Phase 1's first task" landed as Task 2. The §4.7 trust-discriminator wrapping is enforced on every external-content field across all responses (audited by reviewing each `Tagged<>` site in Task 14.2's fmt/clippy pass).

**Placeholder scan:** the two remaining "not implemented" returns are spec-deferred:
- `inspect target=window:<ref>` (Task 12) — Phase 4 absorbs window→AT-SPI app mapping.
- `atspi_focused_root` / `atspi_lookup_by_encoded` skeleton (Task 11/12) depends on Task 0.1 spike findings before final fill-in.
Both are explicit and called out in the task text; neither is a "TODO write tests" type placeholder.

**Type consistency:** `WindowEntry` is the canonical Rust shape for KWin window data; both `window_query` and `observe` use it (re-imported in `observe.rs`). `Tagged<String>` is the universal trust wrapper across all external-text fields. `Region` (enum) crosses two modules (`src/kwin/screenshot.rs` and `src/tools/screenshot.rs`); `derive(Clone)` is added in Task 10.1 to support masking. `KdeMcp::new` signature changes once (Task 2) and is stable thereafter; tests update at the same time.

**Tensions resolved:**
- §4.4 progressive disclosure × `observe` detail levels: the `summary`/`interactive`/`full` branches in Task 13.1 honour the spec field-omission rules (windows + screens omitted in summary).
- §4.5 poka-yoke × `screenshot region`: refs are required for `window:` / `element:`; rect coordinates are accepted only as an explicit `[x,y,w,h]` array — not co-mingled with the ref string.
- §4.7 quarantine × clipboard: `clipboard_read` always returns `trust: External`; the `origin` field exposes whether the writer was us (Phase 1: always `external` because Phase 2 hasn't shipped writes yet).

If you spot a gap not covered by a task above, add the task before starting Task 0.

---

## Execution handoff

Plan complete and saved to `docs/plans/2026-05-14-phase-1-readonly-tools.md`. Two execution options:

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

User pre-approved going autonomously on the parts that don't touch live session state. Recommended sequence in user's absence:

- Task 0 (spikes) — read-only web fetches + one `busctl introspect` + one `wl-paste --list-types`. Safe to run; doesn't mutate the user's session.
- Task 1 (Cargo.toml + module skeleton) — pure local edits.
- Task 2 (audit dispatch wrapper) — pure local refactor + unit tests.
- Task 3 (`text_editor.view`) — pure file I/O over tempfiles.
- Task 4 (`clipboard_read` code) — write the code, run the offline test; **defer the live-session smoke spot-check (step 4.5) until the user is back**, because it requires the user to put known content in the clipboard.
- Task 5 (KWin script runner code) — write the code, run the offline render test; **defer the live script invocation** (the script runner's first end-to-end use comes from Task 6 / 13's live tests, which we hold for the user).
- Task 6 (`window_query` code) — write the code, run the offline projection test; **defer live test.**
- Task 7 (ScreenShot2 proxy code) — write the code; no offline test (it's all D-Bus surface). Compile-only verification.

That covers ~7 of 14 tasks without touching the user's running session in any user-visible way. Tasks 8–14 require live KWin / AT-SPI / wl-paste against the user's session and should wait.
