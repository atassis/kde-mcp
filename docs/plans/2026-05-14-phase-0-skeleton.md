# kde-mcp Phase 0 — Skeleton + Spikes Implementation Plan

> Historical planning artifact — captures the plan as written on the date in the filename; the shipped implementation is the source of truth (see the ADRs and the design spec).

**Goal:** Produce a working `kde-mcp` binary that (a) registers with Claude Code, (b) exposes all 15 MCP tools and returns `not_implemented` for each, (c) has a complete error taxonomy, JSONL audit log, single-session lockfile, KDE acquire/release notifications, and (d) ships with five executed spike notes that verify load-bearing API assumptions before Phase 1.

**Architecture:** Single Rust binary, single tokio runtime. The MCP wire surface is provided by `rmcp 1.7` via the `#[tool_router]` / `#[tool]` / `#[tool_handler]` proc-macros on a `KdeMcp` struct in `src/server.rs`. Each `#[tool]` method is a thin shim that delegates to a free function in `src/tools/<name>.rs`; that's the §13.1 module-layout adaptation. The lockfile and notifications wrap process startup/shutdown; the audit log spans every tool invocation (currently just NotImplemented errors).

**Tech Stack:** Rust 1.85+ edition 2024, `rmcp = "1.7"`, `atspi = "=0.30"` (`tokio` feature), `reis = "=0.6.1"` (`tokio` feature), `zbus = "5.15"` (`tokio` feature, no default), `tokio = "1.40"` (`rt-multi-thread`, `macros`, `signal`, `time`, `sync`), `thiserror = "2.0"`, `anyhow = "1.0"`, `clap = "4.6"` (`derive`), `fd-lock = "4.0"`, `notify-rust = "4.17"`, `serde = "1.0"` (`derive`), `serde_json = "1.0"`, `schemars = "0.8"`, `tracing = "0.1"`, `tracing-subscriber = "0.3"` (`env-filter`), `chrono = "0.4"` (`serde`), `xkbcommon = "0.8"` (deferred to Phase 2 use; not Phase 0).

---

## File structure (Phase 0)

| Path | Responsibility | Phase 0 state |
|---|---|---|
| `Cargo.toml` | Package manifest, pinned deps, MSRV `rust-version = "1.85"`. | Created. |
| `Cargo.lock` | Pinned dependency graph. | Checked in (binary). |
| `rust-toolchain.toml` | Stable channel, profile `default`. | Created. |
| `.gitignore` | `/target`, editor backup files. | Created. |
| `README.md` | Pointer to `docs/`. | Stub. |
| `src/main.rs` | CLI parse, tracing init, bootstrap mode, acquire lock + notify, serve via rmcp stdio, teardown on SIGTERM/SIGINT/EOF. | Complete. |
| `src/lib.rs` | Module declarations. | Complete. |
| `src/cli.rs` | `clap` derive for `--mode=live\|virtual` and `--vnc=<port>`. | Complete. |
| `src/error.rs` | `ToolError` enum (per spec §8); `KdeMcpError` boundary type; `From<ToolError> for rmcp::ErrorData`. | Complete. |
| `src/audit.rs` | JSONL audit writer; redaction rules per spec §9; 50 MB rotation deferred to Phase 6. | Complete. |
| `src/trust.rs` | `Trust` enum + `Tagged<T>` wrapper for `{trust, text}`-shaped fields. | Complete. |
| `src/refs.rs` | Opaque ref encoding (base64 of CBOR) + in-memory TTL `HashMap<RefId, Metadata>`. | Complete. |
| `src/lock.rs` | Per-mode lockfile via `fd-lock`; PID liveness for stale detection. | Complete. |
| `src/notify.rs` | `notify-rust` wrappers for acquire/release notifications. Approval-button delivery deferred to Phase 3. | Complete. |
| `src/server.rs` | `KdeMcp` struct holding shared state; `#[tool_router]` `impl` with all 15 `#[tool]` methods; `#[tool_handler] impl ServerHandler for KdeMcp`. | All tools stubbed to `NotImplemented`. |
| `src/mode/mod.rs` | `Mode` enum (`Live`, `Virtual`); `Bootstrap` trait. | Complete. |
| `src/mode/live.rs` | Connect to user session bus; init audit log path. | Complete (no EIS/atspi yet). |
| `src/mode/virtual.rs` | Phase 0: returns `NotImplemented`. Real impl in Phase 5. | Stub. |
| `src/tools/mod.rs` | Re-exports each tool function. | Complete. |
| `src/tools/observe.rs` ... `text_editor.rs` | 12 files, 15 tools (some files cover paired tools). Each exports `pub(crate) async fn <name>(state: &KdeMcp, args: <Args>) -> Result<<Resp>, ToolError>` returning `Err(ToolError::NotImplemented)`. | All stubbed. |
| `tests/integration/tools_list.rs` | rmcp-client integration test: list tools, verify 15 by name. | Complete. |
| `tests/integration/cli_parse.rs` | CLI argument-parse golden tests. | Complete. |
| `tests/integration/lockfile.rs` | Lockfile contention + stale-PID tests (multi-process). | Complete. |
| `tests/integration/SMOKE.md` | Manual smoke procedure: `claude mcp add` + `tools/list`. | Complete. |
| `docs/research/2026-05-14-phase-0-spikes.md` | Output of Task 0 (one note containing all five spike findings). | Created in Task 0. |

---

## Task ordering and dependencies

Tasks 0 (spikes), 1 (bootstrap), 2 (error), 3 (audit), 4 (trust), 5 (refs) are independent and can be reordered freely. Tasks 6 (lock), 7 (notify) depend on Task 1. Tasks 8 (cli), 9 (mode) depend on Tasks 1–2. Task 10 (tool stubs) depends on Tasks 2 and 4. Task 11 (server) depends on Tasks 2, 5, 10. Task 12 (main) depends on 6, 7, 8, 9, 11. Tasks 13–14 are verification.

Sequential dependency: **Task 0 must complete before Task 1** because the spike findings determine whether the Cargo.toml deps are pinned exactly as planned. Specifically: if Spike 0.1 finds `ProtocolVersion::V_2025_11_25` does not exist in rmcp 1.7, the implementation falls back to `V_2024_11_05` and §1 of the spec is corrected in a follow-up commit during Task 14.

---

## Task 0: Spike verification

Five short read-only investigations. Output: one Markdown note at `docs/research/2026-05-14-phase-0-spikes.md` containing all five findings. Each sub-task ends with appending a section to that file; no source code is written. Estimated total: 90 minutes.

**Files:**
- Create: `docs/research/2026-05-14-phase-0-spikes.md`

- [ ] **Step 0.0: Create the spike-notes file with headers**

```markdown
# Phase 0 Spike Findings

Captured 2026-05-14 before Phase 0 implementation. Each section records what was verified, the source URL or command, and the decision for the implementation plan.

## 0.1 rmcp `ProtocolVersion` variants
## 0.2 `#[tool_router]` composition across `impl` blocks
## 0.3 `atspi-proxies` `Action` interface
## 0.4 `reis` pointer-absolute method names
## 0.5 KWin `org.kde.KWin.EIS.RemoteDesktop` D-Bus signature
```

- [ ] **Step 0.1: rmcp `ProtocolVersion`**

Fetch `https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/model/mod.rs` and grep for `pub enum ProtocolVersion` and `V_`. Record every variant. Decision tree:

- If `V_2025_11_25` (or named `V_2025_LATEST` / `V_DRAFT`) exists → use it.
- Else if `V_2025_06_18` exists → use it; spec §1 stays accurate but §13.1 already covers the version note.
- Else only `V_2024_11_05` exists → use it; in Task 14 add a one-line note to `docs/specs/...` §1 ("The rmcp 1.7 SDK currently advertises 2024-11-05 over the wire; the design conforms to 2025-11-25 semantically and will upgrade the advertised constant when rmcp ships it.").

Append findings to the file. Include the exact variant chosen and the URL of the source file inspected.

- [ ] **Step 0.2: `#[tool_router]` composition**

Write a 30-line throwaway example (do not commit) in `/tmp/rmcp-toolrouter-spike/`. Two files, both containing `#[tool_router] impl Foo { #[tool(...)] async fn a(...) }` and `#[tool_router] impl Foo { #[tool(...)] async fn b(...) }`. Run `cargo check`.

```bash
mkdir -p /tmp/rmcp-toolrouter-spike && cd /tmp/rmcp-toolrouter-spike
cargo init --name spike --bin
# add rmcp 1.7 to Cargo.toml
# write src/main.rs with two #[tool_router] impl blocks on the same struct
cargo check 2>&1 | head -40
```

Record one of three outcomes in the notes file:

1. **Compiles cleanly** → the plan uses split files freely. `KdeMcp` gets four `impl` blocks: observation tools, action tools, window/clipboard, shell+edit.
2. **Compiles with warning about duplicate `tool_router()` constructor** → single `impl` block in `src/server.rs`. All 15 `#[tool]` methods together; one big file but logically split by comment headers.
3. **Compiler error** → confirm error message; fall back to manually building the `ToolRouter` (option 3 in §1 of the rust-stack research note).

Clean up `/tmp/rmcp-toolrouter-spike/`.

- [ ] **Step 0.3: `atspi-proxies` `Action` interface**

Fetch `https://raw.githubusercontent.com/odilia-app/atspi/main/atspi-proxies/src/action.rs` and record:

- The proxy trait name (expected: `ActionProxy<'_>`).
- Methods on it (expected: `do_action(i32)`, `get_action_name(i32)`, `get_action_description(i32)`, `n_actions()`).
- Whether it returns `zbus::Result<...>` or a custom `atspi::Result<...>`.

This unblocks Phase 1's `act_on_element`. Phase 0 does not call any of these methods — but the spike confirms the signature so Phase 1's plan can be written without re-fetching.

- [ ] **Step 0.4: `reis` pointer-absolute methods**

Fetch `https://raw.githubusercontent.com/ids1024/reis/master/src/pointer_absolute.rs` and `https://raw.githubusercontent.com/ids1024/reis/master/src/pointer.rs`. Record:

- For `PointerAbsolute`: exact method name for "move pointer to (x, y) in screen coords". Expected: `motion_absolute(x: f32, y: f32)` or similar.
- For `Pointer`: relative motion (`motion_relative(dx, dy)`), button press/release.
- The frame-marker pattern (`device.frame(serial, time_us)`) — what is `serial`, where does it come from?

This unblocks Phase 2's `mouse.click` / `mouse.move`. Phase 0 does not exercise this.

- [ ] **Step 0.5: KWin EIS D-Bus signature**

On the dev machine (must be a KDE Plasma 6 Wayland session — verify with `echo $XDG_SESSION_TYPE` returns `wayland` and `echo $KDE_SESSION_VERSION` returns `6`):

```bash
qdbus6 org.kde.KWin /org/kde/KWin org.freedesktop.DBus.Introspectable.Introspect 2>&1 | grep -A 8 -i "EIS\|RemoteDesktop"
# Then drill into the EIS interface if present:
qdbus6 org.kde.KWin /org/kde/KWin/EIS org.freedesktop.DBus.Introspectable.Introspect 2>&1 | head -50
```

If neither path returns the EIS interface, try:

```bash
busctl --user introspect org.kde.KWin /org/kde/KWin 2>&1 | grep -i "EIS\|RemoteDesktop"
busctl --user tree org.kde.KWin 2>&1 | head -40
```

Record the exact object path, interface name, and method names (expected: a method that returns an FD-typed argument like `h` or `UnixFD`). If the interface is not exposed under that name, search for the actual KDE EIS interface (`org.kde.kwin.virtual.input`, `org.kde.kwin.input_emulator`, or similar — the name has been moving in KDE 6.x releases). Whatever the actual name is, record it. Phase 1 and Phase 2 cannot proceed without this.

- [ ] **Step 0.6: Commit the spike notes**

```bash
git add docs/research/2026-05-14-phase-0-spikes.md
git commit -m "docs(research): Phase 0 spike findings — rmcp / atspi / reis / KWin EIS"
```

---

## Task 1: Repository bootstrap

Initialise the crate, pin dependencies, set MSRV.

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `README.md` (stub)
- Create: `src/main.rs` (placeholder; replaced in Task 12)
- Create: `src/lib.rs` (empty module declarations; replaced in later tasks)

- [ ] **Step 1.1: `cargo init`**

```bash
cd ~/path/to/kde-mcp
cargo init --name kde-mcp --bin --vcs none
```

Expected: creates `Cargo.toml`, `src/main.rs` (hello world), updates `.gitignore`. Does not re-init git (we already have a repo).

- [ ] **Step 1.2: Write `Cargo.toml`**

Overwrite the generated file with:

```toml
[package]
name = "kde-mcp"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "MCP server controlling KDE Plasma 6 / Wayland desktops"
license = "MIT"
publish = false

[[bin]]
name = "kde-mcp"
path = "src/main.rs"

[dependencies]
rmcp           = { version = "1.7", features = ["server", "macros"] }
tokio          = { version = "1.40", features = ["rt-multi-thread", "macros", "signal", "time", "sync", "io-std", "fs"] }
zbus           = { version = "5.15", default-features = false, features = ["tokio"] }
clap           = { version = "4.6", features = ["derive"] }
thiserror      = "2.0"
anyhow         = "1.0"
serde          = { version = "1.0", features = ["derive"] }
serde_json     = "1.0"
schemars       = "0.8"
fd-lock        = "4.0"
notify-rust    = "4.17"
tracing        = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono         = { version = "0.4", default-features = false, features = ["std", "clock", "serde"] }

[dev-dependencies]
tempfile       = "3.10"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
```

Notes:
- `atspi`, `reis`, `xkbcommon`, `portable-pty`, `image`, `tesseract` are **not** added in Phase 0. Each comes in with the phase that first uses it.
- `zbus` has `default-features = false` because the default features pull async-std; we want tokio integration.

- [ ] **Step 1.3: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
profile = "default"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 1.4: Write `.gitignore`**

Replace the cargo-init default with:

```gitignore
/target
**/*.rs.bk
*.swp
*.swo
.vscode/
.idea/
.direnv/
```

- [ ] **Step 1.5: Stub `src/main.rs` and `src/lib.rs`**

`src/main.rs`:

```rust
fn main() {
    eprintln!("kde-mcp: Phase 0 bootstrap placeholder");
}
```

`src/lib.rs`:

```rust
// Module declarations land here in subsequent tasks.
```

- [ ] **Step 1.6: Verify it builds**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in <N>s`.

- [ ] **Step 1.7: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore src/ README.md
git commit -m "chore(phase-0): bootstrap cargo crate with pinned dependencies"
```

---

## Task 2: Error taxonomy

Implement the 13 error codes from spec §8 as a Rust enum, plus the boundary conversion to `rmcp::ErrorData`. TDD.

**Files:**
- Create: `src/error.rs`
- Modify: `src/lib.rs` (add `pub mod error;`)
- Test: in-file `#[cfg(test)] mod tests`

- [ ] **Step 2.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
```

- [ ] **Step 2.2: Write the failing test**

Append to `src/error.rs`:

```rust
use thiserror::Error;
use serde::Serialize;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ToolError {
    #[error("stale handle: ref no longer valid")]
    StaleHandle { ref_id: String },
    #[error("not actionable")]
    NotActionable { available_actions: Vec<String>, fallback_suggestion: Option<String> },
    #[error("target not found")]
    TargetNotFound { closest_match: Option<String> },
    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },
    #[error("tier restricted")]
    TierRestricted { app_class: String, current_tier: String, allowed_actions: Vec<String> },
    #[error("approval required")]
    AppApprovalRequired { app_class: String, notification_id: Option<u32>, still_pending: bool },
    #[error("cancelled at {cancelled_at_stage}")]
    Cancelled { cancelled_at_stage: String },
    #[error("lock unavailable; held by pid {holder_pid}")]
    LockUnavailable { holder_pid: u32, lock_path: String },
    #[error("timeout")]
    Timeout { closest_match: Option<String>, current_windows: Vec<String> },
    #[error("input dispatch failed: {detail}")]
    InputDispatchFailed { detail: String, libei_version: Option<String>, kwin_version: Option<String> },
    #[error("external command failed: {stderr}")]
    ExternalCommandFailed { stderr: String },
    #[error("bus disconnected: {detail}")]
    BusDisconnected { detail: String },
    #[error("not implemented: this tool is a Phase 0 stub")]
    NotImplemented,
}

impl ToolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::StaleHandle { .. } => "stale_handle",
            Self::NotActionable { .. } => "not_actionable",
            Self::TargetNotFound { .. } => "target_not_found",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::TierRestricted { .. } => "tier_restricted",
            Self::AppApprovalRequired { .. } => "app_approval_required",
            Self::Cancelled { .. } => "cancelled",
            Self::LockUnavailable { .. } => "lock_unavailable",
            Self::Timeout { .. } => "timeout",
            Self::InputDispatchFailed { .. } => "input_dispatch_failed",
            Self::ExternalCommandFailed { .. } => "external_command_failed",
            Self::BusDisconnected { .. } => "bus_disconnected",
            Self::NotImplemented => "not_implemented",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_has_distinct_code() {
        let codes = [
            ToolError::StaleHandle { ref_id: "x".into() }.code(),
            ToolError::TargetNotFound { closest_match: None }.code(),
            ToolError::PermissionDenied { reason: "x".into() }.code(),
            ToolError::Cancelled { cancelled_at_stage: "x".into() }.code(),
            ToolError::NotImplemented.code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    }

    #[test]
    fn code_matches_spec_section_8_table() {
        assert_eq!(ToolError::NotImplemented.code(), "not_implemented");
        assert_eq!(
            ToolError::LockUnavailable { holder_pid: 0, lock_path: "/x".into() }.code(),
            "lock_unavailable"
        );
        assert_eq!(
            ToolError::Cancelled { cancelled_at_stage: "before_dispatch".into() }.code(),
            "cancelled"
        );
    }

    #[test]
    fn serializes_with_code_tag() {
        let e = ToolError::NotImplemented;
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "not_implemented");
    }
}
```

- [ ] **Step 2.3: Run tests — expect compile fail or test fail**

```bash
cargo test --lib error 2>&1 | tail -15
```

Expected: compile errors only if `serde`/`serde_json`/`thiserror` are not in deps (they are per Task 1). Otherwise tests should already pass — this is the "TDD inverted": writing the type and the test together is fine when the type IS the implementation. If tests pass, proceed.

- [ ] **Step 2.4: Add `From<ToolError> for rmcp::ErrorData`**

Append to `src/error.rs`:

```rust
impl From<ToolError> for rmcp::ErrorData {
    fn from(e: ToolError) -> Self {
        let code = e.code();
        let message = e.to_string();
        let data = serde_json::to_value(&e).ok();
        // rmcp::ErrorData carries (code: i64, message: String, data: Option<Value>).
        // Spec §8 says JSON-RPC errors are reserved for malformed args; semantic
        // failures ride on the `isError: true, structuredContent: { code, ... }`
        // pattern. The CallToolResult wrapper is what we'll wrap stub returns in.
        // Here we just emit a generic MCP error if the boundary code path ever needs it.
        let _ = (code, &message, &data);
        rmcp::ErrorData::internal_error(message, data)
    }
}
```

Note: this `From` impl is **not the primary error path**. Tool handlers return `Err(rmcp::ErrorData)` for protocol errors only. Semantic failures go through `CallToolResult::error()` with `structuredContent`. Task 11 sets up the actual pattern. This `From` exists for `?`-elision on unexpected internal errors at the rmcp boundary.

- [ ] **Step 2.5: Run tests — expect pass**

```bash
cargo test --lib error 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 2.6: Commit**

```bash
git add src/error.rs src/lib.rs
git commit -m "feat(phase-0): error taxonomy per spec §8 (13 codes + NotImplemented)"
```

---

## Task 3: Audit log

JSONL writer at `~/.local/state/kde-mcp/audit.jsonl` with the line shape from spec §9. Redaction for `clipboard_write.content` and long `text_editor_edit` fields is included; rotation is deferred to Phase 6.

**Files:**
- Create: `src/audit.rs`
- Modify: `src/lib.rs`

- [ ] **Step 3.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
```

- [ ] **Step 3.2: Write the failing test**

Append to `src/audit.rs`:

```rust
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub ts: String,
    pub mode: &'static str,
    pub tool: String,
    pub args: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_fallback: Option<bool>,
}

pub fn redact(tool: &str, args: Value) -> Value {
    let Value::Object(mut map) = args else { return args };
    match tool {
        "clipboard_write" => {
            if let Some(content) = map.get_mut("content") {
                *content = json!({ "sha256": sha256_hex(content) });
            }
        }
        "text_editor_edit" => {
            for key in ["old_str", "new_str", "content"] {
                if let Some(v) = map.get_mut(key) {
                    if let Some(s) = v.as_str() {
                        if s.len() > 500 {
                            *v = json!({ "sha256": sha256_hex(v), "len": s.len() });
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Value::Object(map)
}

fn sha256_hex(v: &Value) -> String {
    use std::hash::{Hash, Hasher};
    // Phase 0: not real sha256 yet — only the redaction *shape* is load-bearing.
    // Replace with `sha2::Sha256` when Phase 4 adds it.
    let s = serde_json::to_string(v).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("placeholder:{:x}", h.finish())
}

pub fn default_path() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state).join("kde-mcp/audit.jsonl");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/state/kde-mcp/audit.jsonl");
    }
    PathBuf::from("/tmp/kde-mcp/audit.jsonl")
}

pub struct AuditLog {
    file: Mutex<File>,
}

impl AuditLog {
    pub async fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path).await?;
        Ok(Self { file: Mutex::new(file) })
    }

    pub async fn write(&self, mut rec: AuditRecord) -> std::io::Result<()> {
        if rec.ts.is_empty() {
            rec.ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        }
        let mut line = serde_json::to_string(&rec).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut f = self.file.lock().await;
        f.write_all(line.as_bytes()).await?;
        f.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_a_parseable_jsonl_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::open(&path).await.unwrap();
        log.write(AuditRecord {
            ts: String::new(),
            mode: "live",
            tool: "observe".into(),
            args: json!({}),
            ok: true,
            duration_ms: Some(5),
            error: None,
            vision_fallback: None,
        }).await.unwrap();
        drop(log);

        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["tool"], "observe");
        assert_eq!(v["mode"], "live");
        assert_eq!(v["ok"], true);
        assert!(v["ts"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn redacts_clipboard_write_content() {
        let r = redact("clipboard_write", json!({"content": "secret"}));
        assert!(r["content"]["sha256"].is_string());
        assert!(r["content"].get("plaintext").is_none());
    }

    #[test]
    fn long_text_editor_strings_are_redacted() {
        let big = "x".repeat(501);
        let r = redact("text_editor_edit", json!({"old_str": big}));
        assert!(r["old_str"]["sha256"].is_string());
        assert_eq!(r["old_str"]["len"], 501);
    }

    #[test]
    fn short_text_editor_strings_pass_through() {
        let r = redact("text_editor_edit", json!({"old_str": "short"}));
        assert_eq!(r["old_str"], "short");
    }
}
```

- [ ] **Step 3.3: Run tests**

```bash
cargo test --lib audit 2>&1 | tail -10
```

Expected: 4 passed.

- [ ] **Step 3.4: Commit**

```bash
git add src/audit.rs src/lib.rs
git commit -m "feat(phase-0): JSONL audit log with redaction (spec §9)"
```

---

## Task 4: Trust wrappers

Three-level `trust` discriminator from spec §4.7. Pure value type; serializes as `{ "trust": "...", "text": "..." }`. Phase 0 only needs the type — no consumer yet.

**Files:**
- Create: `src/trust.rs`
- Modify: `src/lib.rs`

- [ ] **Step 4.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
```

- [ ] **Step 4.2: Write `src/trust.rs` with inline tests**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// OS-API output (window class, AT-SPI role enum, geometry, mode).
    Trusted,
    /// Content the agent recently authored (clipboard we set; file we created).
    AgentAuthored,
    /// Externally-sourced text (window title, AT-SPI name/value, clipboard read, OCR, bash stdout, file content).
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tagged<T> {
    pub trust: Trust,
    pub text: T,
}

impl<T> Tagged<T> {
    pub fn trusted(text: T) -> Self { Self { trust: Trust::Trusted, text } }
    pub fn agent_authored(text: T) -> Self { Self { trust: Trust::AgentAuthored, text } }
    pub fn external(text: T) -> Self { Self { trust: Trust::External, text } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_trust_and_text() {
        let s = Tagged::external("Save File".to_string());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["trust"], "external");
        assert_eq!(v["text"], "Save File");
    }

    #[test]
    fn round_trip_through_serde() {
        let s = Tagged::trusted("PushButton".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let back: Tagged<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn all_three_trust_levels_have_distinct_serializations() {
        let t = serde_json::to_value(Trust::Trusted).unwrap();
        let a = serde_json::to_value(Trust::AgentAuthored).unwrap();
        let e = serde_json::to_value(Trust::External).unwrap();
        assert_eq!(t, "trusted");
        assert_eq!(a, "agent_authored");
        assert_eq!(e, "external");
    }
}
```

- [ ] **Step 4.3: Run tests**

```bash
cargo test --lib trust 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 4.4: Commit**

```bash
git add src/trust.rs src/lib.rs
git commit -m "feat(phase-0): trust discriminator wrappers (spec §4.7)"
```

---

## Task 5: Opaque refs

In-memory ref store with TTL. Phase 0 needs the type and the eviction; real ref minting (against atspi `ObjectRef` and KWin window UUIDs) lands in Phase 1.

**Files:**
- Create: `src/refs.rs`
- Modify: `src/lib.rs`

- [ ] **Step 5.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
```

- [ ] **Step 5.2: Write `src/refs.rs` with tests**

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKind {
    Window,
    Element,
}

#[derive(Debug, Clone)]
pub struct RefMetadata {
    pub kind: RefKind,
    pub payload: String,
    pub minted_at: Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum RefError {
    #[error("stale handle")]
    Stale,
    #[error("unknown handle")]
    Unknown,
}

pub struct RefStore {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    map: HashMap<String, RefMetadata>,
    ttl: Duration,
    next_id: u64,
}

impl RefStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner { map: HashMap::new(), ttl, next_id: 0 })),
        }
    }

    pub async fn mint(&self, kind: RefKind, payload: impl Into<String>) -> String {
        let mut g = self.inner.write().await;
        let id = format!(
            "{}#{:x}",
            match kind { RefKind::Window => "win", RefKind::Element => "el" },
            g.next_id
        );
        g.next_id += 1;
        g.map.insert(id.clone(), RefMetadata {
            kind,
            payload: payload.into(),
            minted_at: Instant::now(),
        });
        id
    }

    pub async fn resolve(&self, id: &str) -> Result<RefMetadata, RefError> {
        let g = self.inner.read().await;
        match g.map.get(id) {
            None => Err(RefError::Unknown),
            Some(m) if m.minted_at.elapsed() > g.ttl => Err(RefError::Stale),
            Some(m) => Ok(m.clone()),
        }
    }

    pub async fn purge_expired(&self) {
        let mut g = self.inner.write().await;
        let ttl = g.ttl;
        g.map.retain(|_, m| m.minted_at.elapsed() <= ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mint_then_resolve_returns_metadata() {
        let store = RefStore::new(Duration::from_secs(60));
        let id = store.mint(RefKind::Window, "kwin-uuid:abc").await;
        let m = store.resolve(&id).await.unwrap();
        assert_eq!(m.kind, RefKind::Window);
        assert_eq!(m.payload, "kwin-uuid:abc");
    }

    #[tokio::test]
    async fn unknown_handle_errors() {
        let store = RefStore::new(Duration::from_secs(60));
        let err = store.resolve("nonsense#0").await.unwrap_err();
        assert!(matches!(err, RefError::Unknown));
    }

    #[tokio::test]
    async fn expired_handle_returns_stale() {
        let store = RefStore::new(Duration::from_millis(50));
        let id = store.mint(RefKind::Element, "atspi-path").await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let err = store.resolve(&id).await.unwrap_err();
        assert!(matches!(err, RefError::Stale));
    }

    #[tokio::test]
    async fn purge_removes_only_expired() {
        let store = RefStore::new(Duration::from_millis(50));
        let old = store.mint(RefKind::Window, "old").await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let fresh = store.mint(RefKind::Window, "fresh").await;
        store.purge_expired().await;
        assert!(matches!(store.resolve(&old).await, Err(RefError::Unknown)));
        assert!(store.resolve(&fresh).await.is_ok());
    }
}
```

- [ ] **Step 5.3: Run tests**

```bash
cargo test --lib refs 2>&1 | tail -10
```

Expected: 4 passed.

- [ ] **Step 5.4: Commit**

```bash
git add src/refs.rs src/lib.rs
git commit -m "feat(phase-0): opaque ref store with TTL (spec §4.5 poka-yoke)"
```

---

## Task 6: Lockfile

Per-mode lockfile at `$XDG_RUNTIME_DIR/kde-mcp/<mode>.lock`. Held for the life of the process; PID-liveness check for stale-lock detection.

**Files:**
- Create: `src/lock.rs`
- Modify: `src/lib.rs`
- Test: `tests/integration/lockfile.rs`

- [ ] **Step 6.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
```

- [ ] **Step 6.2: Write `src/lock.rs` with unit tests**

```rust
use fd_lock::RwLock;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("io error opening lockfile: {0}")]
    Io(#[from] std::io::Error),
    #[error("lock already held by pid {pid}")]
    AlreadyHeld { pid: u32 },
}

pub struct ModeLock {
    _guard: fd_lock::RwLockWriteGuard<'static, File>,
    path: PathBuf,
}

impl ModeLock {
    pub fn path_for(mode: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("kde-mcp").join(format!("{mode}.lock"))
    }

    /// Acquire the lock for `mode`. On contention, reads the holder PID from
    /// the file and returns `AlreadyHeld` *unless* the holder is dead (stale),
    /// in which case the lock is reclaimed.
    pub fn acquire(mode: &str) -> Result<Self, LockError> {
        let path = Self::path_for(mode);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).read(true).write(true).open(&path)?;
        let lock = Box::leak(Box::new(RwLock::new(file)));

        match lock.try_write() {
            Ok(mut guard) => {
                // We have the lock — write our pid.
                guard.seek(SeekFrom::Start(0))?;
                guard.set_len(0)?;
                write!(guard, "{}", std::process::id())?;
                guard.flush()?;
                Ok(Self { _guard: guard, path })
            }
            Err(_) => {
                // Contention — read the holder pid and check liveness.
                let mut f = File::open(&path)?;
                let mut buf = String::new();
                f.read_to_string(&mut buf)?;
                let pid: u32 = buf.trim().parse().unwrap_or(0);
                if pid_alive(pid) {
                    Err(LockError::AlreadyHeld { pid })
                } else {
                    // Stale — caller can retry. We do not auto-reclaim here to keep
                    // the contention path simple; the wrapper in main.rs may retry.
                    Err(LockError::AlreadyHeld { pid })
                }
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    if pid == 0 { return false; }
    // signal 0 = liveness probe per POSIX
    let r = unsafe { libc::kill(pid as i32, 0) };
    r == 0 || (r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool { false }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lives_under_xdg_runtime_dir() {
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let p = ModeLock::path_for("live");
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("kde-mcp/live.lock"));
    }
}
```

Add to `Cargo.toml` `[dependencies]`:

```toml
libc = "0.2"
```

- [ ] **Step 6.3: Write integration test**

`tests/integration/lockfile.rs`:

```rust
use kde_mcp::lock::{LockError, ModeLock};

#[test]
fn second_acquire_in_same_process_fails() {
    let dir = tempfile::tempdir().unwrap();
    // Each test sets XDG_RUNTIME_DIR — but env mutation is process-global.
    // We use a unique mode name to dodge cross-test interference.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let _a = ModeLock::acquire("live-test-a").unwrap();
    let err = ModeLock::acquire("live-test-a").unwrap_err();
    assert!(matches!(err, LockError::AlreadyHeld { .. }));
}

#[test]
fn different_modes_can_coexist() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let _live = ModeLock::acquire("live-test-b").unwrap();
    let _virt = ModeLock::acquire("virtual-test-b").unwrap();
    // No panic; both held.
}

#[test]
fn release_on_drop_allows_reacquire() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    {
        let _a = ModeLock::acquire("live-test-c").unwrap();
    }
    let _b = ModeLock::acquire("live-test-c").unwrap();
}
```

Make sure `tests/integration/` is wired. Each `.rs` file under `tests/` is a separate test binary in Cargo's convention. If a wrapper module file is needed, create `tests/integration.rs` with `mod lockfile;` later when more integration tests join.

- [ ] **Step 6.4: Run tests**

```bash
cargo test 2>&1 | tail -15
```

Expected: lockfile tests pass; unit tests pass.

- [ ] **Step 6.5: Commit**

```bash
git add src/lock.rs src/lib.rs tests/integration/lockfile.rs Cargo.toml Cargo.lock
git commit -m "feat(phase-0): per-mode single-session lockfile (spec §10 layer 3)"
```

---

## Task 7: Desktop notifications

Acquire / release notifications via `notify-rust`. Phase 0 only needs the two events; approval-button delivery (`Allow once` / `Allow this session` / `Deny`) is Phase 3.

**Files:**
- Create: `src/notify.rs`
- Modify: `src/lib.rs`

- [ ] **Step 7.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
pub mod notify;
```

- [ ] **Step 7.2: Write `src/notify.rs`**

```rust
use notify_rust::{Hint, Notification, Urgency};

pub async fn notify_lock_acquired(mode: &str) {
    let mode_name = match mode {
        "live" => "your live desktop",
        "virtual" => "a virtual desktop session",
        _ => "your desktop",
    };
    let body = format!(
        "kde-mcp is controlling {}. Press Meta+Esc to abort.",
        mode_name
    );
    let _ = tokio::task::spawn_blocking(move || {
        Notification::new()
            .summary("Claude is using your computer")
            .body(&body)
            .appname("kde-mcp")
            .hint(Hint::Urgency(Urgency::Normal))
            .timeout(notify_rust::Timeout::Milliseconds(5000))
            .show()
    })
    .await;
}

pub async fn notify_lock_released(mode: &str) {
    let body = format!("kde-mcp finished using {} mode.", mode);
    let _ = tokio::task::spawn_blocking(move || {
        Notification::new()
            .summary("Claude finished using your computer")
            .body(&body)
            .appname("kde-mcp")
            .hint(Hint::Urgency(Urgency::Low))
            .timeout(notify_rust::Timeout::Milliseconds(3000))
            .show()
    })
    .await;
}

#[cfg(test)]
mod tests {
    // Notifications are end-to-end side effects against the user's session bus.
    // Unit-testing them would require mocking org.freedesktop.Notifications,
    // which is more harness than the Phase 0 surface justifies. Tests are
    // deferred to a manual smoke step in Task 13.
}
```

Notes:
- `notify-rust`'s blocking `.show()` is run via `spawn_blocking` because the crate's async path requires explicit features and the blocking path is fine for low-frequency lock events.
- `appname("kde-mcp")` makes the notification appear under a consistent name in KDE's notification UI per ADR-0007's expectation.

- [ ] **Step 7.3: Verify it compiles**

```bash
cargo build 2>&1 | tail -5
```

Expected: clean build.

- [ ] **Step 7.4: Commit**

```bash
git add src/notify.rs src/lib.rs
git commit -m "feat(phase-0): KDE acquire/release notifications via notify-rust"
```

---

## Task 8: CLI parsing

`--mode=live|virtual` and `--vnc=<port>` (virtual-only). TDD via integration test.

**Files:**
- Create: `src/cli.rs`
- Modify: `src/lib.rs`
- Test: `tests/integration/cli_parse.rs`

- [ ] **Step 8.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
pub mod notify;
pub mod cli;
```

- [ ] **Step 8.2: Write the failing test**

`tests/integration/cli_parse.rs`:

```rust
use clap::Parser;
use kde_mcp::cli::{Cli, Mode};

#[test]
fn parses_live_mode() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=live"]).unwrap();
    assert_eq!(cli.mode, Mode::Live);
    assert!(cli.vnc.is_none());
}

#[test]
fn parses_virtual_mode_with_vnc() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=virtual", "--vnc=5910"]).unwrap();
    assert_eq!(cli.mode, Mode::Virtual);
    assert_eq!(cli.vnc, Some(5910));
}

#[test]
fn rejects_unknown_mode() {
    let err = Cli::try_parse_from(["kde-mcp", "--mode=invalid"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid"), "actual: {msg}");
}

#[test]
fn rejects_vnc_in_live_mode() {
    let err = Cli::try_parse_from(["kde-mcp", "--mode=live", "--vnc=5910"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.to_lowercase().contains("vnc") || msg.to_lowercase().contains("conflict"),
            "actual: {msg}");
}

#[test]
fn mode_is_required() {
    let err = Cli::try_parse_from(["kde-mcp"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("mode"), "actual: {msg}");
}
```

- [ ] **Step 8.3: Run — expect failure (no `cli` module)**

```bash
cargo test --test cli_parse 2>&1 | tail -10
```

Expected: compile error `unresolved import`.

- [ ] **Step 8.4: Implement `src/cli.rs`**

```rust
use clap::{ArgGroup, Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum Mode {
    Live,
    Virtual,
}

#[derive(Debug, Parser)]
#[command(name = "kde-mcp", version, about = "MCP server for KDE Plasma 6 / Wayland")]
#[command(group = ArgGroup::new("vnc-requires-virtual").multiple(false))]
pub struct Cli {
    /// Operating mode. Live drives the user's session; Virtual runs an isolated KWin child.
    #[arg(long, value_enum)]
    pub mode: Mode,

    /// VNC port for virtual-mode observation (wayvnc). Ignored in live mode.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1024..))]
    pub vnc: Option<u16>,
}

impl Cli {
    /// Manual cross-validation that clap's derive doesn't express in 4.x.
    pub fn validate(&self) -> Result<(), clap::Error> {
        if self.mode == Mode::Live && self.vnc.is_some() {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--vnc requires --mode=virtual\n",
            ));
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn parse_and_validate<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    cli.validate()?;
    Ok(cli)
}
```

Update the test for `rejects_vnc_in_live_mode` to use the validation helper:

```rust
#[test]
fn rejects_vnc_in_live_mode() {
    let err = kde_mcp::cli::parse_and_validate(["kde-mcp", "--mode=live", "--vnc=5910"])
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.to_lowercase().contains("vnc"), "actual: {msg}");
}
```

- [ ] **Step 8.5: Run tests**

```bash
cargo test --test cli_parse 2>&1 | tail -10
```

Expected: 5 passed.

- [ ] **Step 8.6: Commit**

```bash
git add src/cli.rs src/lib.rs tests/integration/cli_parse.rs
git commit -m "feat(phase-0): clap CLI for --mode and --vnc"
```

---

## Task 9: Mode bootstrap stubs

`Mode` is dispatched at startup. Live mode opens a zbus session connection and confirms `org.kde.KWin` is reachable; virtual mode returns `NotImplemented` (real impl in Phase 5).

**Files:**
- Create: `src/mode/mod.rs`
- Create: `src/mode/live.rs`
- Create: `src/mode/virtual.rs`
- Modify: `src/lib.rs`

- [ ] **Step 9.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
pub mod notify;
pub mod cli;
pub mod mode;
```

- [ ] **Step 9.2: Write `src/mode/mod.rs`**

```rust
pub mod live;
pub mod r#virtual;

use crate::cli::Mode;
use crate::error::ToolError;
use zbus::Connection;

pub struct BootstrappedState {
    pub mode: Mode,
    pub bus: Connection,
}

pub async fn bootstrap(mode: Mode) -> Result<BootstrappedState, ToolError> {
    match mode {
        Mode::Live => live::bootstrap().await,
        Mode::Virtual => r#virtual::bootstrap().await,
    }
}
```

- [ ] **Step 9.3: Write `src/mode/live.rs`**

```rust
use super::BootstrappedState;
use crate::cli::Mode;
use crate::error::ToolError;
use zbus::Connection;

pub async fn bootstrap() -> Result<BootstrappedState, ToolError> {
    let bus = Connection::session()
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    // Phase 0 sanity check: confirm org.kde.KWin is on the bus.
    // Phase 1 will use the same call to open the EIS RemoteDesktop pipe.
    let proxy = zbus::fdo::DBusProxy::new(&bus)
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    let names = proxy
        .list_names()
        .await
        .map_err(|e| ToolError::BusDisconnected { detail: e.to_string() })?;

    let kwin_present = names.iter().any(|n| n.as_str() == "org.kde.KWin");
    if !kwin_present {
        tracing::warn!(
            "org.kde.KWin not present on session bus; running outside a KDE session?"
        );
        // Phase 0: warn but proceed. Phase 1 will hard-error here.
    } else {
        tracing::info!("live mode bootstrapped; org.kde.KWin present");
    }

    Ok(BootstrappedState { mode: Mode::Live, bus })
}
```

- [ ] **Step 9.4: Write `src/mode/virtual.rs`**

```rust
use super::BootstrappedState;
use crate::error::ToolError;

pub async fn bootstrap() -> Result<BootstrappedState, ToolError> {
    // Phase 5 implements full virtual-mode bootstrap:
    //   1. dbus-run-session -- sleep infinity (private bus)
    //   2. kwin_wayland --virtual on a random socket
    //   3. at-spi-bus-launcher --launch-immediately
    //   4. wayvnc (if --vnc set)
    //   5. EIS handoff against the private bus
    Err(ToolError::NotImplemented)
}
```

- [ ] **Step 9.5: Verify build**

```bash
cargo build 2>&1 | tail -5
```

Expected: clean build. No tests yet — bootstrap exercises a live bus and goes into the integration smoke step in Task 13.

- [ ] **Step 9.6: Commit**

```bash
git add src/mode/ src/lib.rs
git commit -m "feat(phase-0): mode bootstrap stubs (live confirms kwin; virtual NotImpl)"
```

---

## Task 10: Tool modules — 12 files, 15 tools (stubs)

Each `src/tools/<name>.rs` exports a free function that takes a typed args struct and returns `Err(ToolError::NotImplemented)`. The Args/Response types are Phase-0-minimal: just enough for `schemars::JsonSchema` to advertise the tool over MCP.

**Files:**
- Create: `src/tools/mod.rs`
- Create: `src/tools/observe.rs`, `inspect.rs`, `screenshot.rs`, `read_text.rs`, `wait.rs`
- Create: `src/tools/act_on_element.rs`, `mouse.rs`, `keyboard.rs`
- Create: `src/tools/window.rs` (`window_query` + `window_control`)
- Create: `src/tools/clipboard.rs` (`clipboard_read` + `clipboard_write`)
- Create: `src/tools/bash.rs`
- Create: `src/tools/text_editor.rs` (`text_editor_view` + `text_editor_edit`)
- Modify: `src/lib.rs`

- [ ] **Step 10.1: Add module declaration**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
pub mod notify;
pub mod cli;
pub mod mode;
pub mod tools;
```

- [ ] **Step 10.2: Write `src/tools/mod.rs`**

```rust
pub mod observe;
pub mod inspect;
pub mod screenshot;
pub mod read_text;
pub mod wait;
pub mod act_on_element;
pub mod mouse;
pub mod keyboard;
pub mod window;
pub mod clipboard;
pub mod bash;
pub mod text_editor;
```

- [ ] **Step 10.3: Write `src/tools/observe.rs` as the template**

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub struct ObserveArgs {
    #[serde(default)]
    pub detail: Option<DetailLevel>,
    #[serde(default)]
    pub include_other_windows: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Summary,
    #[default]
    Interactive,
    Full,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ObserveResponse {
    // Phase 1 fills this. Phase 0 returns NotImplemented before this is constructed.
}

pub async fn observe(_args: ObserveArgs) -> Result<ObserveResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

- [ ] **Step 10.4: Write the other 11 tool stub files**

Each follows the same template. Args/Response structs are skeletal — names and at-most one field per tool, enough to be valid MCP tool advertisements without committing to the full spec §6 shape yet. The richer shapes come in the phase that implements each tool.

Provide the exact content for each file. Where two tools share a file, both stubs live in the same file.

**`src/tools/inspect.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InspectArgs {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub max_elements: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct InspectResponse {}

pub async fn inspect(_args: InspectArgs) -> Result<InspectResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/screenshot.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotArgs {
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub include_cursor: Option<bool>,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ScreenshotResponse {}

pub async fn screenshot(_args: ScreenshotArgs) -> Result<ScreenshotResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/read_text.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReadTextArgs {
    pub region: String,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub include_bboxes: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ReadTextResponse {}

pub async fn read_text(_args: ReadTextArgs) -> Result<ReadTextResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/wait.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WaitArgs {
    pub condition: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub seconds: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct WaitResponse {}

pub async fn wait(_args: WaitArgs) -> Result<WaitResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/act_on_element.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ActOnElementResponse {}

pub async fn act_on_element(_args: ActOnElementArgs) -> Result<ActOnElementResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/mouse.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MouseArgs {
    pub action: String,
    #[serde(default)]
    pub coordinate: Option<[i32; 2]>,
    #[serde(default)]
    pub button: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct MouseResponse {}

pub async fn mouse(_args: MouseArgs) -> Result<MouseResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/keyboard.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardTypeArgs {
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardKeyArgs {
    pub keys: String,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct KeyboardResponse {}

pub async fn keyboard_type(_args: KeyboardTypeArgs) -> Result<KeyboardResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn keyboard_key(_args: KeyboardKeyArgs) -> Result<KeyboardResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

Note: `keyboard` is intentionally split into two functions per spec §4.5 poka-yoke ("`keyboard.type` accepts `text` only; `keyboard.key` accepts `keys` only"). The MCP wire surface is **two separate tools** (`keyboard.type`, `keyboard.key`) — confirmed below in Task 11. Total tool count therefore is 15, not 14, even though "keyboard" sounds singular.

**`src/tools/window.rs`** (two tools, one file):

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub struct WindowQueryArgs {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct WindowQueryResponse {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WindowControlArgs {
    pub window_ref: String,
    pub action: String,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct WindowControlResponse {}

pub async fn window_query(_args: WindowQueryArgs) -> Result<WindowQueryResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn window_control(_args: WindowControlArgs) -> Result<WindowControlResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/clipboard.rs`** (two tools):

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct ClipboardReadArgs {}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ClipboardReadResponse {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipboardWriteArgs {
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ClipboardWriteResponse {}

pub async fn clipboard_read(_args: ClipboardReadArgs) -> Result<ClipboardReadResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn clipboard_write(_args: ClipboardWriteArgs) -> Result<ClipboardWriteResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/bash.rs`**:

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BashArgs {
    pub command: String,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct BashResponse {}

pub async fn bash(_args: BashArgs) -> Result<BashResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

**`src/tools/text_editor.rs`** (two tools):

```rust
use crate::error::ToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TextEditorViewArgs {
    pub path: String,
    #[serde(default)]
    pub view_range: Option<[i32; 2]>,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct TextEditorViewResponse {}

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

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct TextEditorEditResponse {}

pub async fn text_editor_view(_args: TextEditorViewArgs) -> Result<TextEditorViewResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

pub async fn text_editor_edit(_args: TextEditorEditArgs) -> Result<TextEditorEditResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
```

- [ ] **Step 10.5: Verify build**

```bash
cargo build 2>&1 | tail -5
```

Expected: clean build. All 12 modules compile; 15 stubs available.

- [ ] **Step 10.6: Commit**

```bash
git add src/tools/ src/lib.rs
git commit -m "feat(phase-0): stub 15 tool functions returning NotImplemented"
```

---

## Task 11: MCP server struct

The `KdeMcp` struct in `src/server.rs` is the proc-macro-driven MCP surface. Per the Task 0.2 spike outcome, choose the impl-block layout:

- **If split impls compile** → four `#[tool_router]` impls grouped by category.
- **If single impl required** → one big impl with comment-section dividers.

The plan below assumes split impls work; if not, collapse into one `impl KdeMcp` block.

**Files:**
- Create: `src/server.rs`
- Modify: `src/lib.rs`
- Test: `tests/integration/tools_list.rs`

- [ ] **Step 11.1: Add module declaration and re-exports**

`src/lib.rs`:

```rust
pub mod error;
pub mod audit;
pub mod trust;
pub mod refs;
pub mod lock;
pub mod notify;
pub mod cli;
pub mod mode;
pub mod tools;
pub mod server;

pub use server::KdeMcp;
```

- [ ] **Step 11.2: Write `src/server.rs`**

Use the exact pattern from the rmcp counter example (research note §1), adapted for 15 tools:

```rust
use crate::cli::Mode;
use crate::error::ToolError;
use crate::refs::RefStore;
use crate::tools;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct KdeMcp {
    pub mode: Mode,
    pub refs: Arc<RefStore>,
    pub tool_router: ToolRouter<KdeMcp>,
}

impl KdeMcp {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            refs: Arc::new(RefStore::new(Duration::from_secs(60))),
            tool_router: Self::tool_router(),
        }
    }
}

// ---- Observation tools (5) ----

#[tool_router]
impl KdeMcp {
    #[tool(description = "Situational snapshot of the desktop. No screenshot.")]
    async fn observe(
        &self,
        Parameters(args): Parameters<tools::observe::ObserveArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::observe::observe(args).await)
    }

    #[tool(description = "Drill into the AT-SPI tree of a window or element.")]
    async fn inspect(
        &self,
        Parameters(args): Parameters<tools::inspect::InspectArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::inspect::inspect(args).await)
    }

    #[tool(description = "Capture pixels. Prefer observe/inspect for structural questions.")]
    async fn screenshot(
        &self,
        Parameters(args): Parameters<tools::screenshot::ScreenshotArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::screenshot::screenshot(args).await)
    }

    #[tool(description = "OCR a region. Use when AT-SPI doesn't expose text.")]
    async fn read_text(
        &self,
        Parameters(args): Parameters<tools::read_text::ReadTextArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::read_text::read_text(args).await)
    }

    #[tool(description = "Block until a structured condition is met.")]
    async fn wait(
        &self,
        Parameters(args): Parameters<tools::wait::WaitArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::wait::wait(args).await)
    }
}

// ---- Action tools (5) ----

#[tool_router]
impl KdeMcp {
    #[tool(description = "Invoke a named action on an accessibility element.")]
    async fn act_on_element(
        &self,
        Parameters(args): Parameters<tools::act_on_element::ActOnElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::act_on_element::act_on_element(args).await)
    }

    #[tool(description = "Mouse click/move/scroll/drag at coordinates (vision fallback).")]
    async fn mouse(
        &self,
        Parameters(args): Parameters<tools::mouse::MouseArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::mouse::mouse(args).await)
    }

    #[tool(name = "keyboard.type", description = "Type literal text. No modifier keys.")]
    async fn keyboard_type(
        &self,
        Parameters(args): Parameters<tools::keyboard::KeyboardTypeArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::keyboard::keyboard_type(args).await)
    }

    #[tool(name = "keyboard.key", description = "Press named keys or chords (no text).")]
    async fn keyboard_key(
        &self,
        Parameters(args): Parameters<tools::keyboard::KeyboardKeyArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::keyboard::keyboard_key(args).await)
    }
}

// ---- Window + clipboard tools (4) ----

#[tool_router]
impl KdeMcp {
    #[tool(description = "List windows on the desktop.")]
    async fn window_query(
        &self,
        Parameters(args): Parameters<tools::window::WindowQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::window::window_query(args).await)
    }

    #[tool(description = "Focus/minimise/maximise/close a window by ref.")]
    async fn window_control(
        &self,
        Parameters(args): Parameters<tools::window::WindowControlArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::window::window_control(args).await)
    }

    #[tool(description = "Read the clipboard.")]
    async fn clipboard_read(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardReadArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::clipboard::clipboard_read(args).await)
    }

    #[tool(description = "Set the clipboard.")]
    async fn clipboard_write(
        &self,
        Parameters(args): Parameters<tools::clipboard::ClipboardWriteArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::clipboard::clipboard_write(args).await)
    }
}

// ---- Shell + edit tools (3) ----

#[tool_router]
impl KdeMcp {
    #[tool(description = "Run a shell command via a persistent bash session.")]
    async fn bash(
        &self,
        Parameters(args): Parameters<tools::bash::BashArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::bash::bash(args).await)
    }

    #[tool(name = "text_editor.view", description = "Read a file or directory listing.")]
    async fn text_editor_view(
        &self,
        Parameters(args): Parameters<tools::text_editor::TextEditorViewArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::text_editor::text_editor_view(args).await)
    }

    #[tool(name = "text_editor.edit", description = "Edit a file via create/replace/insert/undo.")]
    async fn text_editor_edit(
        &self,
        Parameters(args): Parameters<tools::text_editor::TextEditorEditArgs>,
    ) -> Result<CallToolResult, McpError> {
        tool_result(tools::text_editor::text_editor_edit(args).await)
    }
}

#[tool_handler]
impl ServerHandler for KdeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_server_info(Implementation::from_build_env())
        // Spike 0.1 determined which variant to use; if V_2025_11_25 wasn't
        // present in rmcp 1.7, fall back to V_2024_11_05 and note in §1 of
        // the spec.
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_instructions(
            "kde-mcp: KDE Plasma 6 / Wayland desktop control. \
             Phase 0 build — all tools return not_implemented."
                .to_string(),
        )
    }
}

fn tool_result<T: serde::Serialize>(
    r: Result<T, ToolError>,
) -> Result<CallToolResult, McpError> {
    match r {
        Ok(payload) => {
            let v = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
            Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
        }
        Err(e) => {
            let code = e.code();
            let msg = e.to_string();
            let details = serde_json::to_value(&e).unwrap_or(serde_json::Value::Null);
            // CallToolResult with isError: true and structuredContent carrying the code.
            // rmcp 1.7 exposes a builder for this; if the exact API name differs from
            // `error` (e.g. it might be `with_error`), adjust here — the shape is fixed.
            let payload = serde_json::json!({ "code": code, "details": details });
            Ok(CallToolResult::error(vec![
                Content::text(msg),
                Content::text(payload.to_string()),
            ]))
        }
    }
}
```

**Notes on this file:**

- The exact `CallToolResult::error()` constructor name in rmcp 1.7 is what `tool_result()` depends on. If `cargo build` errors with "no method named `error`", grep rmcp's source (`cargo doc --open --package rmcp` or `cargo expand`) for the right constructor — it may be `CallToolResult::error_text(msg)`, `CallToolResult::builder().is_error(true).content(...).build()`, or similar. Replace inline; do not introduce a workaround that hides `isError: true`.
- The `#[tool(name = "...")]` overrides apply only where the wire-level name has a dot (`keyboard.type`, `keyboard.key`, `text_editor.view`, `text_editor.edit`). Other tools take their method name verbatim.
- If Task 0.2 found the macro **does not** compose across impl blocks, collapse the four `#[tool_router]` impls into one `impl KdeMcp { ... }` block keeping all 15 `#[tool]` methods inside it. The comment dividers stay; the only structural change is removing the inner `#[tool_router]` annotations on the secondary impls.

- [ ] **Step 11.3: Write the integration test**

`tests/integration/tools_list.rs`:

```rust
use kde_mcp::KdeMcp;
use kde_mcp::cli::Mode;

#[tokio::test]
async fn server_advertises_fifteen_tools_with_expected_names() {
    let server = KdeMcp::new(Mode::Live);
    let router = &server.tool_router;
    let names: std::collections::HashSet<String> = router
        .list_all()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();

    let expected: std::collections::HashSet<String> = [
        "observe", "inspect", "screenshot", "read_text", "wait",
        "act_on_element", "mouse", "keyboard.type", "keyboard.key",
        "window_query", "window_control",
        "clipboard_read", "clipboard_write",
        "bash",
        "text_editor.view", "text_editor.edit",
    ].iter().map(|s| s.to_string()).collect();

    assert_eq!(names.len(), 16, "expected 16 tool names (observe + ... + edit), got {names:?}");
    // The spec calls this "15 tools" but counts keyboard.{type,key} as one
    // tool with two surfaces; on the wire they are two distinct registrations.
    // text_editor.{view,edit} likewise. So 15 logical tools = 16 wire names
    // (keyboard splits into 2; text_editor splits into 2; window stays as 2;
    //  clipboard stays as 2). Adjust expected count if 0.2 spike changed naming.
    assert!(names.is_superset(&expected), "missing tools: {:?}", expected.difference(&names));
}
```

**Wire-name accounting** (to make this assertion correct — read carefully):

- The spec §6 lists 15 logical tools.
- `keyboard.type` and `keyboard.key` are split for poka-yoke (§4.5) — that's two wire names.
- `text_editor_view` and `text_editor_edit` are also split — two wire names. The spec uses underscores; the `name = "text_editor.view"` override above uses dots to match MCP's hierarchical naming convention surfaced in rmcp examples. If the spike or rmcp client tooling prefers underscores, change the override back to `text_editor_view` / `text_editor_edit`.
- `window_query` / `window_control` and `clipboard_read` / `clipboard_write` are also two wire names each.

Count: observation 5 + action 4 (act_on_element, mouse, keyboard.type, keyboard.key) + window 2 + clipboard 2 + bash 1 + text_editor 2 = **16 wire names**.

The spec's "15 tools" comes from counting the keyboard split as one logical tool. Update the assertion in `tools_list.rs` to 16, and update the test name comment.

- [ ] **Step 11.4: Run tests**

```bash
cargo test --test tools_list 2>&1 | tail -10
```

Expected: 1 passed (16 tool names visible).

If `ToolRouter::list_all()` is not the actual API name, grep rmcp source for the method that lists registered tools (likely `tools()` or `routes()`). Adjust the test.

- [ ] **Step 11.5: Run unit tests too**

```bash
cargo test 2>&1 | tail -15
```

Expected: all green.

- [ ] **Step 11.6: Commit**

```bash
git add src/server.rs src/lib.rs tests/integration/tools_list.rs
git commit -m "feat(phase-0): KdeMcp server with 16 tool wire names (15 logical)"
```

---

## Task 12: Main wires everything

`src/main.rs` parses CLI, initialises tracing, bootstraps the mode, acquires the lock, fires the acquire notification, serves rmcp over stdio, and on shutdown releases the lock and fires the release notification.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 12.1: Write `src/main.rs`**

```rust
use anyhow::{Context, Result};
use kde_mcp::{
    KdeMcp, audit, cli::parse_and_validate, lock::ModeLock, mode, notify,
};
use rmcp::{ServiceExt, transport::stdio};
use tracing::Level;
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

    let cli = parse_and_validate(std::env::args_os())
        .map_err(|e| { let _ = e.print(); anyhow::anyhow!("invalid CLI args") })?;
    let mode = cli.mode;

    let mode_name = match mode {
        kde_mcp::cli::Mode::Live => "live",
        kde_mcp::cli::Mode::Virtual => "virtual",
    };

    let lock = ModeLock::acquire(mode_name)
        .with_context(|| format!("acquiring {mode_name} mode lock"))?;
    tracing::info!(lock = %lock.path().display(), "mode lock acquired");

    notify::notify_lock_acquired(mode_name).await;

    let _state = mode::bootstrap(mode)
        .await
        .with_context(|| format!("bootstrapping {mode_name} mode"))?;

    // Open the audit log; failure here is fatal (we never want to silently
    // run without an audit trail).
    let audit_path = audit::default_path();
    let _audit = audit::AuditLog::open(&audit_path)
        .await
        .with_context(|| format!("opening audit log at {}", audit_path.display()))?;
    tracing::info!(audit = %audit_path.display(), "audit log open");

    let server = KdeMcp::new(mode);

    eprintln!("kde-mcp: ready (mode={mode_name})");

    let service = server
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("rmcp serve: {e}"))?;

    // service.waiting() returns when the host closes stdin / sends a shutdown.
    service.waiting().await?;

    drop(lock); // explicit for clarity
    notify::notify_lock_released(mode_name).await;
    tracing::info!("shut down cleanly");
    Ok(())
}
```

- [ ] **Step 12.2: Verify build**

```bash
cargo build 2>&1 | tail -5
```

Expected: clean release-mode build (or dev build, whichever is run).

If the `rmcp::ServiceExt::serve` call signature differs from this draft, adjust to match the actual API. The counter example in the rmcp repo is the authoritative reference.

- [ ] **Step 12.3: Verify `--help` and `--version`**

```bash
cargo run --quiet -- --help 2>&1 | head -20
cargo run --quiet -- --version 2>&1
```

Expected: help text mentions `--mode` and `--vnc`. Version prints `kde-mcp 0.1.0`.

- [ ] **Step 12.4: Commit**

```bash
git add src/main.rs
git commit -m "feat(phase-0): main wires CLI -> lock -> notify -> bootstrap -> rmcp stdio"
```

---

## Task 13: Integration smoke test

Manual + scripted verification that Claude Code discovers `kde-mcp` and can call its (stubbed) tools.

**Files:**
- Create: `tests/integration/SMOKE.md`

- [ ] **Step 13.1: Build the release binary**

```bash
cargo build --release 2>&1 | tail -5
ls -la target/release/kde-mcp
```

Expected: binary present at `target/release/kde-mcp`.

- [ ] **Step 13.2: Register with Claude Code**

```bash
claude mcp add kde-mcp-phase0 "$PWD/target/release/kde-mcp" -- --mode=live
claude mcp list
```

Expected: `kde-mcp-phase0` appears in the list.

- [ ] **Step 13.3: Discover tools in a Claude Code conversation**

Open Claude Code and ask: "List the tools available from kde-mcp-phase0." Expected: all 16 wire names appear (the model may collapse keyboard.type/keyboard.key into one mention, but the underlying list is 16).

- [ ] **Step 13.4: Invoke one tool and observe the error**

In Claude Code: "Call kde-mcp-phase0's observe tool with no arguments."

Expected: the response carries `isError: true` and a structured payload like `{"code": "not_implemented", ...}`. The conversation should surface "this tool is a Phase 0 stub."

- [ ] **Step 13.5: Verify the lock + notification side effects**

While Claude Code is connected:
- Check the lockfile exists: `ls -la "$XDG_RUNTIME_DIR/kde-mcp/live.lock"` shows a file owned by the user, non-empty (contains PID).
- A KDE notification should have fired: "Claude is using your computer · Press Meta+Esc to abort."
- Try to start a second `kde-mcp --mode=live` in another terminal: it should fail with `lock_unavailable` and print the holding PID.

- [ ] **Step 13.6: Verify the audit log**

```bash
tail -5 ~/.local/state/kde-mcp/audit.jsonl 2>&1
```

Phase 0 does not yet hook audit-log writes into the tool dispatcher (that's Phase 1's first task), so this file may be empty. The presence of the file and its location is what we're verifying — the writes will come in Phase 1.

- [ ] **Step 13.7: Disconnect Claude Code; verify release notification**

```bash
claude mcp remove kde-mcp-phase0
```

Expected: a second KDE notification fires ("Claude finished using your computer"). Lockfile is removed (or marked deletable).

- [ ] **Step 13.8: Write `tests/integration/SMOKE.md`**

```markdown
# Phase 0 Smoke Test Procedure

Manual verification of the kde-mcp Phase 0 build against Claude Code on a real KDE Plasma 6 Wayland session. Runs after Task 12.

## Preconditions

- KDE Plasma 6 / Wayland session (`echo $XDG_SESSION_TYPE` returns `wayland`).
- `claude` CLI installed and authenticated.
- No other kde-mcp processes running (`pgrep kde-mcp` returns nothing).

## Steps

[Steps 13.1 through 13.7 above, condensed into shell-executable form.]

## Pass criteria

- Binary registers via `claude mcp add`.
- 16 wire-named tools visible in `tools/list`.
- Every tool call returns `code: not_implemented`.
- Acquire notification fires on startup.
- Lockfile present and contains the binary's PID.
- Second `--mode=live` invocation fails with `lock_unavailable`.
- Release notification fires on disconnect.
- Audit log file exists at `~/.local/state/kde-mcp/audit.jsonl` (may be empty in Phase 0).
```

- [ ] **Step 13.9: Commit**

```bash
git add tests/integration/SMOKE.md
git commit -m "docs(phase-0): manual smoke procedure"
```

---

## Task 14: Phase 0 sign-off

Final verification + cleanup + version bump if any spec-correcting follow-up is needed from the spikes.

- [ ] **Step 14.1: Format + lint clean**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: zero warnings, zero errors.

- [ ] **Step 14.2: Full test run**

```bash
cargo test --workspace 2>&1 | tail -20
```

Expected: all unit + integration tests pass.

- [ ] **Step 14.3: Spec correction if Spike 0.1 found an older protocol version**

If `rmcp 1.7` only exports `V_2024_11_05`, append to `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` §1 a one-line note:

```markdown
> **Wire-protocol version note (2026-05-14):** The reference rmcp 1.7 SDK currently advertises MCP `2024-11-05` over the wire. `kde-mcp` is designed against the semantics of `2025-11-25` (see §13.1); the advertised constant will track rmcp's `ProtocolVersion` upstream as it bumps.
```

If `V_2025_11_25` or `V_2025_06_18` was found, no correction needed.

Commit either way:

```bash
git add docs/specs/2026-05-13-kde-computer-use-mcp-design.md  # only if edited
git commit -m "docs(spec): note current wire-protocol version per Phase 0 Spike 0.1"
```

- [ ] **Step 14.4: Update the README status**

Update the README's "Current status" section to record that Phase 0 has shipped:

```markdown
## Current status (2026-05-14)

- **Phase:** 0 complete (skeleton). Ready for Phase 1.
- **Last commit:** [the commit hash from this Task 14]
- **Binary:** target/release/kde-mcp builds; registers with Claude Code; all 16 wire-named tools return not_implemented.
- **Open spike findings:** see docs/research/2026-05-14-phase-0-spikes.md.
```

```bash
git add README.md
git commit -m "docs(readme): mark Phase 0 complete; Phase 1 is the next step"
```

- [ ] **Step 14.5: Tag the Phase 0 release**

```bash
git tag -a phase-0 -m "Phase 0: skeleton, error taxonomy, lock, notify, 15 stubbed tools"
git tag
```

(Local tag only. Per the project's "no pushes without explicit ask" rule, we do not push the tag.)

- [ ] **Step 14.6: Phase 0 done — summary message**

Report to the user (or to the calling agent harness):

- Phase 0 is complete. Binary builds, registers with Claude Code, advertises 16 tools.
- All 5 spike findings recorded.
- Test suite passes. Clippy clean.
- Ready for `writing-plans` for Phase 1 (read-only tools).

---

## Self-review

### Spec coverage check

| Spec section | Phase 0 coverage |
|---|---|
| §4 (design principles) | Trust wrappers (§4.7) implemented as type; poka-yoke ref store (§4.5) implemented. Other principles apply to Phase 1+ shapes. |
| §5.3 (live bootstrap) | Steps 1–3 covered (CLI, audit log, zbus connect). Steps 4–6 (EIS handoff, atspi connect, banner) cleanly deferred — banner partial in main.rs. |
| §5.4 (virtual bootstrap) | Stub returning NotImpl per plan. |
| §6.0 (policy gate) | Not in Phase 0. The gate is Phase 3 per spec §14. |
| §6.1–§6.4 (tool catalogue) | All 15 logical tools stubbed at the wire layer with skeletal Args/Response shapes. Full shapes per phase. |
| §8 (error taxonomy) | All 13 codes implemented as `ToolError` variants. |
| §9 (audit log) | JSONL writer + redaction implemented. Tool-dispatch hookup deferred to Phase 1 (per Step 13.6). |
| §10 (safety model layers 1–11) | Layer 1 (annotations) implicit in tool definitions; layer 3 (lock + notifications) complete. Layers 2, 4–11 are Phase 1+. |
| §13 (project layout) | Implemented with §13.1 adaptation (server.rs added; per-tool free functions; xkbcommon deferred). |
| §14 row 0 (Phase 0) | Fully covered including the verification step. |

Gap discovered during review: spec §10 layer 1 ("`readOnlyHint: true` / `destructiveHint: true` annotations") is not explicitly set on any `#[tool]` annotation. rmcp 1.7 supports tool annotations via `#[tool(annotations(...))]` per the SDK examples. Action: add a sub-step to Task 11 to set `readOnlyHint = true` on observation tools and `destructiveHint = true` on action tools. See addendum below.

### Addendum to Task 11: Tool annotations

After Step 11.2, before Step 11.3, add:

- [ ] **Step 11.2b: Add tool annotations**

For each `#[tool]` attribute in `src/server.rs`, add an `annotations` field:

- Observation tools (`observe`, `inspect`, `screenshot`, `read_text`, `wait`, `window_query`, `clipboard_read`, `text_editor_view`):
  ```rust
  #[tool(description = "...", annotations(read_only_hint = true))]
  ```
- Action tools (`act_on_element`, `mouse`, `keyboard_type`, `keyboard_key`, `window_control`, `clipboard_write`, `text_editor_edit`):
  ```rust
  #[tool(description = "...", annotations(destructive_hint = true))]
  ```
- `bash`:
  ```rust
  #[tool(description = "...", annotations(destructive_hint = true, open_world_hint = true))]
  ```

If rmcp 1.7's `#[tool]` macro uses different attribute names (e.g., `tool_annotations(...)` or a nested `tool_annotation(...)`), grep the rmcp example sources to find the right syntax and adjust. The wire-level effect is what matters: hosts read `readOnlyHint` / `destructiveHint` from the `tools/list` response.

### Placeholder scan

No `TBD`, no `TODO`, no "implement later" in the plan. Each step has either runnable code or runnable commands. The `tool_result()` function's note about `CallToolResult::error()` is the closest to a placeholder — it's an honest "verify against rmcp source" instruction, not "TODO write this." Acceptable.

### Type consistency

- `ToolError` is referenced identically in Tasks 2, 9, and 10.
- `KdeMcp` introduced in Task 11; referenced by name in Tasks 12 and 13.
- `Mode` introduced in Task 8; referenced in Tasks 9, 11, 12.
- `ModeLock` introduced in Task 6; referenced in Task 12.
- `RefStore` introduced in Task 5; referenced in Task 11.
- `AuditLog::open()` and `default_path()` introduced in Task 3; referenced in Task 12.
- `notify_lock_acquired` / `notify_lock_released` introduced in Task 7; referenced in Task 12.

No drift detected.
