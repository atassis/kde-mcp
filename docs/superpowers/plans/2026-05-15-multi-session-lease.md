# Multi-session mutation lease — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the spec §11.3 startup `<mode>.lock` with a per-session lease (ADR-0011) so multiple concurrent Claude Code sessions can coexist, with mutual exclusion enforced at the workflow level — not at process startup.

**Architecture:** A `LeaseStore` rooted at `$XDG_RUNTIME_DIR/kde-mcp/lease.json` and guarded by a POSIX flock. Every tool dispatch classifies as `Mutate` or `Observe`. `Mutate` acquires-or-refreshes the lease (blocking up to `acquire_timeout`, default 60s, returning `busy` on timeout). `Observe` refreshes the lease only if the caller already holds it. Holder-process death is detected via `kill -0` on read; idle expiry is detected lazily on the next acquire attempt. No new daemon, no transport change, no MCP session-id wrangling.

**Tech Stack:** Rust 2024, tokio 1.40, fd-lock 4 (already in deps), chrono 0.4 (already in deps), serde+serde_json. ADR-0011 is the spec; this plan executes it.

---

## File Structure

**Create:**
- `src/lease.rs` — `LeaseConfig`, `LeaseRecord`, `LeaseStore`, `ToolKind`, `AcquireError`, `classify_tool()`. ~250 LOC including doc comments.
- `tests/integration/lease.rs` — integration tests against a tempdir-rooted lease. ~150 LOC.

**Modify:**
- `src/lib.rs` — add `pub mod lease;`; remove `pub mod lock;` after Task 13.
- `src/error.rs` — replace `LockUnavailable` with `Busy`.
- `src/server.rs` — add `lease: Arc<LeaseStore>` to `KdeMcp`; extend `dispatch_audited` with `tool_kind: ToolKind` + lease check at every call site.
- `src/main.rs` — remove `ModeLock::acquire` + `notify_lock_*`; construct `LeaseStore`; install graceful-shutdown release-on-exit hook.
- `src/notify.rs` — replace `notify_lock_acquired/released` with `notify_lease_acquired/released_debounced`.
- `tests/integration.rs` — `mod lease;`, remove `mod lockfile;`.
- `tests/integration/lockfile.rs` — **delete** (lock removed).
- `src/lock.rs` — **delete** (replaced by `src/lease.rs`).
- `docs/specs/2026-05-13-kde-computer-use-mcp-design.md` — §6 step 1, §8 error table, §11.3 layer-3 entry.
- `docs/runbook/KNOWN-FIXES.md` — retire F3 (`lock_unavailable`); add row for `busy`.
- `tests/integration/SMOKE.md` — replace "Concurrent-launch contention" block with the lease-coexistence check.

**Out of scope for this plan (follow-up):**
- KGlobalAccel Meta+Esc handoff between leaseholders. Today every kde-mcp process registers on startup; this is suboptimal but not broken — KGlobalAccel is idempotent for the same component, and Meta+Esc cancelling all sessions matches the "panic button" interpretation. Tracked as a follow-up ADR-0011 amendment.

---

## Task 1: Lease module scaffolding

Wire up empty files so `cargo check` passes; subsequent tasks fill them in TDD-style.

**Files:**
- Create: `src/lease.rs`
- Modify: `src/lib.rs`
- Create: `tests/integration/lease.rs`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Create `src/lease.rs` with a module header.**

```rust
//! Per-session mutation lease (ADR-0011).
//!
//! Replaces the startup `<mode>.lock` (spec §11.3) with workflow-level
//! mutual exclusion. Mutating tools acquire the lease at dispatch; observing
//! tools refresh it only if the caller already holds it. The lease lives at
//! `$XDG_RUNTIME_DIR/kde-mcp/lease.json` and is guarded by POSIX flock.
//!
//! See `docs/adr/0011-multi-session-lease.md` for the design rationale.
```

- [ ] **Step 2: Register the module in `src/lib.rs`.**

In `src/lib.rs`, add a `pub mod lease;` line. Keep `pub mod lock;` for now — it goes away in Task 13.

```rust
pub mod lease;
```

- [ ] **Step 3: Create `tests/integration/lease.rs` with a placeholder.**

```rust
//! Integration tests for the multi-session lease (ADR-0011).
//!
//! Each test sets `XDG_RUNTIME_DIR` to a fresh tempdir so concurrent test
//! runs don't share lease files.

// Tests added in subsequent tasks.
```

- [ ] **Step 4: Declare the integration test module.**

In `tests/integration.rs`, inside the `mod integration { ... }` block, add `pub mod lease;` alongside the existing entries (alphabetic order; insert after `keymap`).

- [ ] **Step 5: Verify the scaffold compiles.**

Run: `cargo check --workspace --all-targets`
Expected: succeeds with no errors (warnings about unused module are fine).

- [ ] **Step 6: Commit.**

```bash
git add src/lease.rs src/lib.rs tests/integration/lease.rs tests/integration.rs
git commit -m "chore(lease): scaffold lease module + integration test wiring"
```

---

## Task 2: `LeaseConfig` and `LeaseRecord` types

Define the on-disk shape and the configuration knobs. TDD via serde round-trip.

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write a failing test for `LeaseConfig::default()`.**

Append to `tests/integration/lease.rs`:

```rust
use kde_mcp::lease::LeaseConfig;
use std::time::Duration;

#[test]
fn default_config_matches_adr_0011() {
    let c = LeaseConfig::default();
    assert_eq!(c.idle_timeout, Duration::from_secs(10));
    assert_eq!(c.acquire_timeout, Duration::from_secs(60));
}
```

- [ ] **Step 2: Run the test and confirm it fails to compile.**

Run: `cargo test --test integration integration::lease::default_config_matches_adr_0011`
Expected: compile error — `LeaseConfig` not found.

- [ ] **Step 3: Implement `LeaseConfig` in `src/lease.rs`.**

Append:

```rust
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct LeaseConfig {
    /// How long after the holder's most recent call the lease stays held
    /// before it idle-expires. Default 10s (ADR-0011).
    pub idle_timeout: Duration,
    /// How long an acquire call blocks waiting for the lease before
    /// returning `busy`. Default 60s.
    pub acquire_timeout: Duration,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(10),
            acquire_timeout: Duration::from_secs(60),
        }
    }
}
```

- [ ] **Step 4: Run the test, confirm it passes.**

Run: `cargo test --test integration integration::lease::default_config_matches_adr_0011`
Expected: PASS.

- [ ] **Step 5: Write a failing test for `LeaseRecord` JSON round-trip.**

Append to `tests/integration/lease.rs`:

```rust
use chrono::{TimeZone, Utc};
use kde_mcp::lease::LeaseRecord;

#[test]
fn record_round_trips_through_json() {
    let r = LeaseRecord {
        holder_pid: 4242,
        holder_session_id: "01J5N0VYE5J9F1V3Q9SHV4Z9P8".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 0).unwrap(),
        last_refresh_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 7).unwrap(),
        expires_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 17).unwrap(),
    };
    let s = serde_json::to_string(&r).unwrap();
    let parsed: LeaseRecord = serde_json::from_str(&s).unwrap();
    assert_eq!(parsed, r);
}
```

- [ ] **Step 6: Run the test, confirm compile failure.**

Run: `cargo test --test integration integration::lease::record_round_trips_through_json`
Expected: compile error — `LeaseRecord` not found.

- [ ] **Step 7: Implement `LeaseRecord`.**

Append to `src/lease.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub holder_pid: u32,
    pub holder_session_id: String,
    pub holder_intent: String,
    pub acquired_at: DateTime<Utc>,
    pub last_refresh_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

- [ ] **Step 8: Run the test, confirm it passes.**

Run: `cargo test --test integration integration::lease`
Expected: 2 tests PASS.

- [ ] **Step 9: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): LeaseConfig defaults + LeaseRecord serde shape"
```

---

## Task 3: Lease file path resolution

Mirror the existing `ModeLock::path_for` behavior: `$XDG_RUNTIME_DIR/kde-mcp/lease.json`, fall back to `/tmp`.

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write the failing test.**

Append to `tests/integration/lease.rs`:

```rust
use kde_mcp::lease::lease_path;

#[test]
fn lease_path_lives_under_xdg_runtime_dir() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let p = lease_path();
    assert!(p.starts_with(dir.path()));
    assert!(p.ends_with("kde-mcp/lease.json"));
}
```

- [ ] **Step 2: Run the test, confirm failure.**

Run: `cargo test --test integration integration::lease::lease_path_lives_under_xdg_runtime_dir`
Expected: compile error.

- [ ] **Step 3: Implement `lease_path`.**

Append to `src/lease.rs`:

```rust
use std::path::PathBuf;

pub fn lease_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("kde-mcp").join("lease.json")
}
```

- [ ] **Step 4: Run the test, confirm PASS.**

Run: `cargo test --test integration integration::lease::lease_path_lives_under_xdg_runtime_dir`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): lease_path resolves under XDG_RUNTIME_DIR"
```

---

## Task 4: `pid_alive` helper

Move/copy the `pid_alive` helper out of `src/lock.rs` into `src/lease.rs` so the lease module is self-contained ahead of `src/lock.rs`'s deletion in Task 13.

**Files:**
- Modify: `src/lease.rs`

- [ ] **Step 1: Add the helper.**

Append to `src/lease.rs`:

```rust
/// Liveness probe — signal 0 returns 0 if the process exists.
/// EPERM also indicates existence (we lack permission to signal).
#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let r = unsafe { libc::kill(pid as i32, 0) };
    if r == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub(crate) fn pid_alive(_pid: u32) -> bool {
    false
}
```

- [ ] **Step 2: Add a unit test.**

Append at the bottom of `src/lease.rs` (in a `#[cfg(test)] mod tests` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_alive_true_for_self_false_for_zero() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }
}
```

- [ ] **Step 3: Run the test, confirm PASS.**

Run: `cargo test --lib lease::tests`
Expected: 1 test PASS.

- [ ] **Step 4: Commit.**

```bash
git add src/lease.rs
git commit -m "feat(lease): pid_alive liveness probe"
```

---

## Task 5: `try_acquire_once` — empty-file path

Acquire a lease on a non-existent (or expired/stale) lease file. Single-shot, non-blocking.

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write the failing test.**

Append to `tests/integration/lease.rs`:

```rust
use kde_mcp::lease::{LeaseStore, lease_path};

#[test]
fn try_acquire_on_empty_file_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let store = LeaseStore::new(
        LeaseConfig::default(),
        std::process::id(),
        "session-A".into(),
    );
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, kde_mcp::lease::TryOutcome::Acquired));
    assert!(lease_path().exists(), "lease file should exist after acquire");
}
```

- [ ] **Step 2: Confirm compile failure.**

Run: `cargo test --test integration integration::lease::try_acquire_on_empty_file_succeeds`
Expected: compile error — `LeaseStore`, `TryOutcome` not defined.

- [ ] **Step 3: Implement `LeaseStore` skeleton + `try_acquire_once` for the empty/stale/expired path.**

Append to `src/lease.rs`:

```rust
use fd_lock::RwLock;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug)]
pub enum TryOutcome {
    Acquired,
    Busy(LeaseRecord),
}

pub struct LeaseStore {
    config: LeaseConfig,
    self_pid: u32,
    self_session: String,
}

impl LeaseStore {
    pub fn new(config: LeaseConfig, self_pid: u32, self_session: String) -> Self {
        Self {
            config,
            self_pid,
            self_session,
        }
    }

    /// Single non-blocking acquire attempt.
    ///
    /// Returns `Acquired` when the lease was previously absent, expired,
    /// stale (holder PID dead), or already ours; in those cases the on-disk
    /// record is rewritten to reflect *us* as the holder. Returns
    /// `Busy(holder)` otherwise.
    pub fn try_acquire_once(&self, intent: &str) -> std::io::Result<TryOutcome> {
        let path = lease_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        let mut lock = RwLock::new(file);
        // Exclusive flock for the duration of the read-modify-write.
        let mut guard = lock.write()?;
        guard.seek(SeekFrom::Start(0))?;
        let mut body = String::new();
        guard.read_to_string(&mut body)?;
        let existing: Option<LeaseRecord> = if body.trim().is_empty() {
            None
        } else {
            serde_json::from_str(&body).ok()
        };
        let now = Utc::now();
        let we_can_take = match &existing {
            None => true,
            Some(r) if r.holder_pid == self.self_pid => true,
            Some(r) if !pid_alive(r.holder_pid) => true,
            Some(r) if r.expires_at <= now => true,
            Some(_) => false,
        };
        if !we_can_take {
            return Ok(TryOutcome::Busy(existing.unwrap()));
        }
        let acquired_at = match &existing {
            Some(r) if r.holder_pid == self.self_pid => r.acquired_at,
            _ => now,
        };
        let rec = LeaseRecord {
            holder_pid: self.self_pid,
            holder_session_id: self.self_session.clone(),
            holder_intent: intent.to_string(),
            acquired_at,
            last_refresh_at: now,
            expires_at: now + chrono::Duration::from_std(self.config.idle_timeout).unwrap(),
        };
        let serialised = serde_json::to_vec(&rec)?;
        guard.seek(SeekFrom::Start(0))?;
        guard.set_len(0)?;
        guard.write_all(&serialised)?;
        guard.flush()?;
        Ok(TryOutcome::Acquired)
    }
}
```

- [ ] **Step 4: Add `use kde_mcp::lease::LeaseConfig;` (if missing) at the top of the test file.**

Check that `tests/integration/lease.rs` has all needed `use` lines for `LeaseConfig`, `LeaseStore`, `TryOutcome`, `lease_path`.

- [ ] **Step 5: Run the test, confirm PASS.**

Run: `cargo test --test integration integration::lease::try_acquire_on_empty_file_succeeds`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): try_acquire_once on empty/expired/stale lease file"
```

---

## Task 6: `try_acquire_once` — busy path (alive holder)

Verify a second store with a different pid sees `Busy` while the first is held.

**Files:**
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write the failing test.**

Append to `tests/integration/lease.rs`:

```rust
#[test]
fn try_acquire_when_alive_other_holder_returns_busy() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    // Holder is *this* process (signal 0 succeeds → alive).
    let store_a = LeaseStore::new(
        LeaseConfig::default(),
        std::process::id(),
        "session-A".into(),
    );
    let _acquired = store_a.try_acquire_once("keyboard.type").unwrap();
    // Pretend we're a different process that is *also* alive (we lie about
    // pid: re-use our own PID + 0 won't work — use a sibling pid by claiming
    // self_pid is u32::MAX but the holder is us).
    //
    // Easier: use a second store whose self_pid is the same process. The
    // `we_can_take` rule "holder_pid == self.self_pid" will then short-
    // circuit to Acquired. So spawn a child that holds the lease, then
    // attempt acquire from this process.
    //
    // Simpler approach: write a lease record manually pointing at PID 1
    // (init — always alive on Linux), then attempt acquire from us.
    let path = lease_path();
    let rec = kde_mcp::lease::LeaseRecord {
        holder_pid: 1, // init, always alive
        holder_session_id: "alien".into(),
        holder_intent: "mouse".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store_b = LeaseStore::new(LeaseConfig::default(), 99999, "session-B".into());
    let outcome = store_b.try_acquire_once("keyboard.type").unwrap();
    match outcome {
        kde_mcp::lease::TryOutcome::Busy(holder) => {
            assert_eq!(holder.holder_pid, 1);
            assert_eq!(holder.holder_intent, "mouse");
        }
        kde_mcp::lease::TryOutcome::Acquired => panic!("expected Busy, got Acquired"),
    }
}
```

- [ ] **Step 2: Run the test, confirm PASS.**

Run: `cargo test --test integration integration::lease::try_acquire_when_alive_other_holder_returns_busy`
Expected: PASS (the implementation from Task 5 already handles this).

- [ ] **Step 3: Add a test for the dead-PID stale-detection branch.**

Append:

```rust
#[test]
fn try_acquire_when_dead_holder_takes_lease() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    // Write a lease pointing at a guaranteed-dead PID.
    //
    // PID 0xFFFFFFE (just under u32::MAX) is well above any plausible
    // running PID — kernel.pid_max defaults to 4194304 on Linux. The kill(0)
    // probe will return ESRCH (no such process), so we treat the lease as
    // stale.
    let path = lease_path();
    let rec = kde_mcp::lease::LeaseRecord {
        holder_pid: 0xFFFFFFE,
        holder_session_id: "ghost".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, kde_mcp::lease::TryOutcome::Acquired));
}
```

- [ ] **Step 4: Run the new test, confirm PASS.**

Run: `cargo test --test integration integration::lease::try_acquire_when_dead_holder_takes_lease`
Expected: PASS.

- [ ] **Step 5: Add a test for the expired-but-alive-holder branch.**

Append:

```rust
#[test]
fn try_acquire_when_expired_lease_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    let rec = kde_mcp::lease::LeaseRecord {
        holder_pid: 1, // init, alive
        holder_session_id: "expired".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: chrono::Utc::now() - chrono::Duration::seconds(60),
        last_refresh_at: chrono::Utc::now() - chrono::Duration::seconds(30),
        expires_at: chrono::Utc::now() - chrono::Duration::seconds(20),
    };
    std::fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, kde_mcp::lease::TryOutcome::Acquired));
}
```

- [ ] **Step 6: Run the new test, confirm PASS.**

Run: `cargo test --test integration integration::lease`
Expected: all lease tests PASS.

- [ ] **Step 7: Commit.**

```bash
git add tests/integration/lease.rs
git commit -m "test(lease): coverage for busy / stale-pid / expired branches"
```

---

## Task 7: `refresh_if_holder` and `release_if_holder`

Holder-only operations: extend the idle clock without acquiring; release the lease on clean shutdown.

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write a failing test for `refresh_if_holder` on a non-holder.**

Append to `tests/integration/lease.rs`:

```rust
#[test]
fn refresh_if_holder_is_no_op_when_not_holder() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    let before = kde_mcp::lease::LeaseRecord {
        holder_pid: 1,
        holder_session_id: "other".into(),
        holder_intent: "mouse".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::write(&path, serde_json::to_string(&before).unwrap()).unwrap();
    let store = LeaseStore::new(LeaseConfig::default(), 99999, "us".into());
    assert!(!store.refresh_if_holder().unwrap());
    // File contents unchanged.
    let body = std::fs::read_to_string(&path).unwrap();
    let after: kde_mcp::lease::LeaseRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(after, before);
}
```

- [ ] **Step 2: Write a failing test for `refresh_if_holder` on the actual holder.**

Append:

```rust
#[test]
fn refresh_if_holder_extends_expires_at() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let _ = store.try_acquire_once("act_on_element").unwrap();
    let body = std::fs::read_to_string(lease_path()).unwrap();
    let before: kde_mcp::lease::LeaseRecord = serde_json::from_str(&body).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(store.refresh_if_holder().unwrap());
    let body = std::fs::read_to_string(lease_path()).unwrap();
    let after: kde_mcp::lease::LeaseRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(after.holder_pid, before.holder_pid);
    assert_eq!(after.acquired_at, before.acquired_at);
    assert!(after.last_refresh_at >= before.last_refresh_at);
    assert!(after.expires_at >= before.expires_at);
}
```

- [ ] **Step 3: Write a failing test for `release_if_holder`.**

Append:

```rust
#[test]
fn release_if_holder_removes_lease_file() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let _ = store.try_acquire_once("clipboard_write").unwrap();
    assert!(lease_path().exists());
    store.release_if_holder().unwrap();
    assert!(!lease_path().exists(), "lease file should be deleted on release");
}
```

- [ ] **Step 4: Implement `refresh_if_holder` and `release_if_holder` on `LeaseStore`.**

Append inside `impl LeaseStore`:

```rust
    /// Extend the idle clock if and only if we currently hold the lease.
    /// Returns true if we refreshed, false otherwise.
    pub fn refresh_if_holder(&self) -> std::io::Result<bool> {
        let path = lease_path();
        if !path.exists() {
            return Ok(false);
        }
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let mut lock = RwLock::new(file);
        let mut guard = lock.write()?;
        guard.seek(SeekFrom::Start(0))?;
        let mut body = String::new();
        guard.read_to_string(&mut body)?;
        let mut rec: LeaseRecord = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(_) => return Ok(false),
        };
        if rec.holder_pid != self.self_pid {
            return Ok(false);
        }
        let now = Utc::now();
        rec.last_refresh_at = now;
        rec.expires_at = now + chrono::Duration::from_std(self.config.idle_timeout).unwrap();
        let serialised = serde_json::to_vec(&rec)?;
        guard.seek(SeekFrom::Start(0))?;
        guard.set_len(0)?;
        guard.write_all(&serialised)?;
        guard.flush()?;
        Ok(true)
    }

    /// Release the lease iff we hold it. Removes the file.
    pub fn release_if_holder(&self) -> std::io::Result<()> {
        let path = lease_path();
        if !path.exists() {
            return Ok(());
        }
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let mut lock = RwLock::new(file);
        let guard = lock.write()?;
        // Read body under flock to confirm ownership before deleting.
        let mut body = String::new();
        let mut g = guard;
        g.seek(SeekFrom::Start(0))?;
        g.read_to_string(&mut body)?;
        let is_ours = serde_json::from_str::<LeaseRecord>(&body)
            .map(|r| r.holder_pid == self.self_pid)
            .unwrap_or(false);
        // Drop the guard *before* unlinking — Linux fine either way, but
        // some kernels and POSIX semantics prefer the file unmapped first.
        drop(g);
        if is_ours {
            // Best-effort: ignore ENOENT if another process raced us.
            let _ = std::fs::remove_file(&path);
        }
        Ok(())
    }
```

- [ ] **Step 5: Run all three new tests, confirm PASS.**

Run: `cargo test --test integration integration::lease`
Expected: all PASS.

- [ ] **Step 6: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): refresh_if_holder + release_if_holder"
```

---

## Task 8: Async `acquire` with polling loop and timeout

`acquire` is the high-level entry: blocks until `Acquired` or `Busy` (timeout). Poll every 100ms.

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write a failing test — short timeout against an active holder.**

Append:

```rust
#[tokio::test]
async fn acquire_returns_busy_after_short_timeout_when_held() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    let rec = kde_mcp::lease::LeaseRecord {
        holder_pid: 1, // alive
        holder_session_id: "blocker".into(),
        holder_intent: "wait".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();

    let cfg = LeaseConfig {
        idle_timeout: std::time::Duration::from_secs(10),
        acquire_timeout: std::time::Duration::from_millis(300),
    };
    let store = LeaseStore::new(cfg, 99999, "us".into());
    let started = std::time::Instant::now();
    let err = store.acquire("keyboard.type").await.unwrap_err();
    let elapsed = started.elapsed();
    assert!(elapsed >= std::time::Duration::from_millis(250));
    assert!(elapsed < std::time::Duration::from_millis(700));
    use kde_mcp::lease::AcquireError;
    match err {
        AcquireError::Busy {
            holder_pid,
            holder_intent,
            waited_ms,
            acquire_timeout_ms,
            ..
        } => {
            assert_eq!(holder_pid, 1);
            assert_eq!(holder_intent, "wait");
            assert!(waited_ms >= 250);
            assert_eq!(acquire_timeout_ms, 300);
        }
        AcquireError::Io(e) => panic!("expected Busy, got Io({e})"),
    }
}
```

- [ ] **Step 2: Write a failing test — acquire succeeds immediately on empty file.**

Append:

```rust
#[tokio::test]
async fn acquire_returns_immediately_when_no_holder() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let store = LeaseStore::new(
        LeaseConfig::default(),
        std::process::id(),
        "us".into(),
    );
    let started = std::time::Instant::now();
    store.acquire("keyboard.type").await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
}
```

- [ ] **Step 3: Implement `AcquireError` and `acquire`.**

Append to `src/lease.rs`:

```rust
#[derive(Debug)]
pub enum AcquireError {
    Busy {
        holder_pid: u32,
        holder_intent: String,
        lease_expires_in_ms: u64,
        waited_ms: u64,
        acquire_timeout_ms: u64,
    },
    Io(std::io::Error),
}

impl From<std::io::Error> for AcquireError {
    fn from(e: std::io::Error) -> Self {
        AcquireError::Io(e)
    }
}

impl LeaseStore {
    /// Async acquire: block (polling at 100ms) until we hold the lease or
    /// `acquire_timeout` elapses. Returns `Busy` on timeout.
    pub async fn acquire(&self, intent: &str) -> Result<(), AcquireError> {
        let started = std::time::Instant::now();
        let poll = std::time::Duration::from_millis(100);
        loop {
            let outcome = self.try_acquire_once(intent)?;
            match outcome {
                TryOutcome::Acquired => return Ok(()),
                TryOutcome::Busy(holder) => {
                    let elapsed = started.elapsed();
                    if elapsed >= self.config.acquire_timeout {
                        let now = Utc::now();
                        let until = (holder.expires_at - now).num_milliseconds().max(0) as u64;
                        return Err(AcquireError::Busy {
                            holder_pid: holder.holder_pid,
                            holder_intent: holder.holder_intent,
                            lease_expires_in_ms: until,
                            waited_ms: elapsed.as_millis() as u64,
                            acquire_timeout_ms: self.config.acquire_timeout.as_millis() as u64,
                        });
                    }
                    tokio::time::sleep(poll).await;
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run both new tests, confirm PASS.**

Run: `cargo test --test integration integration::lease::acquire_returns_busy_after_short_timeout_when_held integration::lease::acquire_returns_immediately_when_no_holder`
Expected: both PASS.

- [ ] **Step 5: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): async acquire with 100ms poll + timeout → Busy"
```

---

## Task 9: `ToolKind` and `classify_tool`

The classifier maps the rmcp wire-name (plus the `wait` args) to `Mutate` or `Observe`. ADR-0011 §"Mutating vs observing tool classification".

**Files:**
- Modify: `src/lease.rs`
- Modify: `tests/integration/lease.rs`

- [ ] **Step 1: Write the failing test.**

Append to `tests/integration/lease.rs`:

```rust
use kde_mcp::lease::{ToolKind, classify_tool};

#[test]
fn observe_tools_classified_as_observe() {
    for name in [
        "observe",
        "inspect",
        "screenshot",
        "read_text",
        "window_query",
        "clipboard_read",
        "text_editor.view",
    ] {
        assert_eq!(
            classify_tool(name, &serde_json::Value::Null),
            ToolKind::Observe,
            "expected {name} to be Observe"
        );
    }
}

#[test]
fn mutate_tools_classified_as_mutate() {
    for name in [
        "act_on_element",
        "mouse",
        "keyboard.type",
        "keyboard.key",
        "window_control",
        "clipboard_write",
        "text_editor.edit",
        "bash",
    ] {
        assert_eq!(
            classify_tool(name, &serde_json::Value::Null),
            ToolKind::Mutate,
            "expected {name} to be Mutate"
        );
    }
}

#[test]
fn wait_classification_depends_on_condition() {
    let seconds = serde_json::json!({ "condition": "seconds", "seconds": 1.0 });
    let appears = serde_json::json!({ "condition": "window_appears" });
    let screen = serde_json::json!({ "condition": "screen_change" });
    assert_eq!(classify_tool("wait", &seconds), ToolKind::Observe);
    assert_eq!(classify_tool("wait", &appears), ToolKind::Mutate);
    assert_eq!(classify_tool("wait", &screen), ToolKind::Mutate);
    // Missing condition → fall back to Observe (callers will fail validation
    // before this point but we don't want to crash on malformed args).
    assert_eq!(
        classify_tool("wait", &serde_json::Value::Null),
        ToolKind::Observe
    );
}
```

- [ ] **Step 2: Implement `ToolKind` and `classify_tool`.**

Append to `src/lease.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Mutate,
    Observe,
}

/// Map a tool's wire-name + args to its lease class per ADR-0011.
pub fn classify_tool(wire_name: &str, args: &serde_json::Value) -> ToolKind {
    match wire_name {
        // Observation: never acquires, only refreshes when held.
        "observe"
        | "inspect"
        | "screenshot"
        | "read_text"
        | "window_query"
        | "clipboard_read"
        | "text_editor.view" => ToolKind::Observe,
        // Mutation: acquires the lease.
        "act_on_element"
        | "mouse"
        | "keyboard.type"
        | "keyboard.key"
        | "window_control"
        | "clipboard_write"
        | "text_editor.edit"
        | "bash" => ToolKind::Mutate,
        // `wait` is mutation-class iff its condition blocks on screen state.
        // `condition: "seconds"` is a pure sleep — Observe.
        "wait" => match args.get("condition").and_then(|v| v.as_str()) {
            Some("seconds") | None => ToolKind::Observe,
            Some(_) => ToolKind::Mutate,
        },
        // Unknown tools default to Observe — safer to under-lease than to
        // block on a tool that has no lease semantics. Loud-fail in audit
        // when this triggers (handled in dispatch_audited).
        _ => ToolKind::Observe,
    }
}
```

- [ ] **Step 3: Run the tests, confirm PASS.**

Run: `cargo test --test integration integration::lease`
Expected: all PASS.

- [ ] **Step 4: Commit.**

```bash
git add src/lease.rs tests/integration/lease.rs
git commit -m "feat(lease): ToolKind classifier per ADR-0011 §classification"
```

---

## Task 10: `Busy` variant on `ToolError`; remove `LockUnavailable`

Make the error code wire-shape match ADR-0011's `busy { holder_pid, holder_intent, lease_expires_in_ms, waited_ms, acquire_timeout_ms }`.

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Edit `ToolError` enum.**

In `src/error.rs`, replace the `LockUnavailable` variant with a `Busy` variant. Locate the existing `LockUnavailable` block and replace it (lines ~39–40):

```rust
    #[error("busy; held by pid {holder_pid} running {holder_intent}")]
    Busy {
        holder_pid: u32,
        holder_intent: String,
        lease_expires_in_ms: u64,
        waited_ms: u64,
        acquire_timeout_ms: u64,
    },
```

- [ ] **Step 2: Update the `code()` match arm.**

Replace `Self::LockUnavailable { .. } => "lock_unavailable",` with:

```rust
            Self::Busy { .. } => "busy",
```

- [ ] **Step 3: Update the `each_variant_has_distinct_code` test.**

In the test, replace the `ToolError::LockUnavailable { … }.code()` entry with:

```rust
            ToolError::Busy {
                holder_pid: 1,
                holder_intent: "keyboard.type".into(),
                lease_expires_in_ms: 0,
                waited_ms: 0,
                acquire_timeout_ms: 0,
            }
            .code(),
```

- [ ] **Step 4: Update the `codes_match_spec_section_8_table` test.**

Replace the `lock_unavailable` assertion with:

```rust
        assert_eq!(
            ToolError::Busy {
                holder_pid: 0,
                holder_intent: "wait".into(),
                lease_expires_in_ms: 0,
                waited_ms: 0,
                acquire_timeout_ms: 0,
            }
            .code(),
            "busy"
        );
```

- [ ] **Step 5: Run the error tests.**

Run: `cargo test --lib error::tests`
Expected: all PASS.

- [ ] **Step 6: Commit.**

```bash
git add src/error.rs
git commit -m "feat(error): replace LockUnavailable with Busy { holder_pid, ... } per ADR-0011"
```

---

## Task 11: `From<AcquireError> for ToolError`

Boundary glue so the dispatch wrapper can `?` the lease into a tool error.

**Files:**
- Modify: `src/lease.rs`

- [ ] **Step 1: Add the impl.**

Append to `src/lease.rs`:

```rust
impl From<AcquireError> for crate::error::ToolError {
    fn from(e: AcquireError) -> Self {
        match e {
            AcquireError::Busy {
                holder_pid,
                holder_intent,
                lease_expires_in_ms,
                waited_ms,
                acquire_timeout_ms,
            } => crate::error::ToolError::Busy {
                holder_pid,
                holder_intent,
                lease_expires_in_ms,
                waited_ms,
                acquire_timeout_ms,
            },
            AcquireError::Io(e) => crate::error::ToolError::Io {
                path: lease_path().to_string_lossy().to_string(),
                error: e.to_string(),
            },
        }
    }
}
```

- [ ] **Step 2: Add a unit test in the existing `src/lease.rs` `tests` mod.**

In the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn busy_acquire_error_maps_to_busy_tool_error() {
        let e = AcquireError::Busy {
            holder_pid: 42,
            holder_intent: "mouse".into(),
            lease_expires_in_ms: 1234,
            waited_ms: 567,
            acquire_timeout_ms: 60000,
        };
        let mapped: crate::error::ToolError = e.into();
        assert_eq!(mapped.code(), "busy");
    }
```

- [ ] **Step 3: Run the test, confirm PASS.**

Run: `cargo test --lib lease::tests`
Expected: both lease unit tests PASS.

- [ ] **Step 4: Commit.**

```bash
git add src/lease.rs
git commit -m "feat(lease): From<AcquireError> for ToolError"
```

---

## Task 12: Wire `LeaseStore` into `KdeMcp` and `dispatch_audited`

Add the field; extend the dispatch wrapper to acquire/refresh per ADR-0011's table.

**Files:**
- Modify: `src/server.rs`

- [ ] **Step 1: Add the `lease` field to `KdeMcp`.**

In `src/server.rs`, in the `pub struct KdeMcp { ... }` definition (after the `tray` field, before `tool_router`):

```rust
    pub lease: Arc<crate::lease::LeaseStore>,
```

- [ ] **Step 2: Initialise the lease in `new_with_cancel`.**

Inside `pub fn new_with_cancel(...)`, alongside `tray = ...`, add:

```rust
        let lease = Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            uuid::Uuid::new_v4().to_string(),
        ));
```

And include `lease,` in the struct literal at the end of `new_with_cancel`.

- [ ] **Step 3: Update the `dispatch_audited` signature.**

Find `pub(crate) async fn dispatch_audited<F, T>(` and update it to:

```rust
pub(crate) async fn dispatch_audited<F, T>(
    audit: Arc<AuditLog>,
    tray: Arc<crate::tray::TrayState>,
    lease: Arc<crate::lease::LeaseStore>,
    tool_kind: crate::lease::ToolKind,
    mode: &'static str,
    tool: &'static str,
    args: serde_json::Value,
    fut: F,
) -> CallToolResult
where
    F: std::future::Future<Output = Result<T, ToolError>>,
    T: serde::Serialize,
{
```

- [ ] **Step 4: Add the lease step at the top of `dispatch_audited`, before `TrayGuard::enter`.**

Replace the existing `let _guard = crate::tray::TrayGuard::enter(&tray, tool);` line and what follows it (down to the audit-record build) with:

```rust
    let lease_held = match tool_kind {
        crate::lease::ToolKind::Mutate => match lease.acquire(tool).await {
            Ok(()) => true,
            Err(e) => {
                let err: ToolError = e.into();
                let code = err.code();
                let msg = err.to_string();
                let details = serde_json::to_value(&err).unwrap_or(serde_json::Value::Null);
                let payload = serde_json::json!({ "code": code, "details": details });
                let r = CallToolResult::error(vec![
                    Content::text(msg),
                    Content::text(payload.to_string()),
                ]);
                let rec = crate::audit::AuditRecord {
                    ts: String::new(),
                    mode,
                    tool: tool.to_string(),
                    args: crate::audit::redact(tool, args),
                    ok: false,
                    duration_ms: Some(0),
                    error: Some(payload),
                    vision_fallback: None,
                };
                if let Err(e) = audit.write(rec).await {
                    tracing::error!(error = %e, tool, "audit write failed");
                }
                return r;
            }
        },
        crate::lease::ToolKind::Observe => {
            // Best-effort refresh; ignore errors (lease file missing is fine).
            let _ = lease.refresh_if_holder();
            false
        }
    };
    let _guard = crate::tray::TrayGuard::enter(&tray, tool);
    let started = std::time::Instant::now();
    let outcome = fut.await;
    let duration_ms = started.elapsed().as_millis() as u64;

    // On cancel, release immediately so the next session can take over.
    if lease_held
        && (tray.cancel.is_cancelled() || matches!(outcome, Err(ToolError::Cancelled { .. })))
    {
        let _ = lease.release_if_holder();
    }
```

(The remainder of `dispatch_audited` — building the `call_result`, redacting args, writing the audit record — stays as it was. The earlier code that did this is now reached via the `outcome` binding.)

- [ ] **Step 5: Update every call site of `dispatch_audited` in `src/server.rs`.**

There are 16 call sites (one per tool). For each, insert two new args after `tray`: `self.lease.clone()` and the `ToolKind`. Examples:

For `observe`:
```rust
        Ok(
            dispatch_audited(
                audit,
                tray,
                self.lease.clone(),
                crate::lease::ToolKind::Observe,
                mode,
                "observe",
                v,
                async move { tools::observe::observe(self, args).await },
            )
            .await,
        )
```

For `keyboard_type` (mutating):
```rust
        Ok(
            dispatch_audited(
                audit,
                tray,
                self.lease.clone(),
                crate::lease::ToolKind::Mutate,
                mode,
                "keyboard.type",
                v,
                async move { tools::keyboard::keyboard_type(self, args).await },
            )
            .await,
        )
```

For `wait`, classify from args:
```rust
        let v = serde_json::to_value(&args).unwrap_or(serde_json::Value::Null);
        let kind = crate::lease::classify_tool("wait", &v);
        let audit = self.audit.clone();
        let mode = self.mode.as_str();
        let tray = self.tray.clone();
        let lease = self.lease.clone();
        Ok(dispatch_audited(
            audit,
            tray,
            lease,
            kind,
            mode,
            "wait",
            v,
            async move { tools::wait::wait(self, args).await },
        )
        .await)
```

Tool-by-tool kind assignment (use these exact `ToolKind` values per call site):
- `observe`, `inspect`, `screenshot`, `read_text`, `window_query`, `clipboard_read`, `text_editor.view` → `ToolKind::Observe`
- `act_on_element`, `mouse`, `keyboard.type`, `keyboard.key`, `window_control`, `clipboard_write`, `bash`, `text_editor.edit` → `ToolKind::Mutate`
- `wait` → computed via `classify_tool("wait", &v)`

- [ ] **Step 6: Update the two existing `dispatch_audited_writes_a_record_on_*` tests.**

In `src/server.rs`'s `tests` mod, both tests instantiate `dispatch_audited` directly with `(audit, tray, "live", "observe", …)`. Update them to supply a lease + kind:

```rust
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let lease = std::sync::Arc::new(crate::lease::LeaseStore::new(
            crate::lease::LeaseConfig::default(),
            std::process::id(),
            "test".into(),
        ));
        // For the success test (observe — read-only):
        let r = dispatch_audited(
            audit.clone(),
            tray,
            lease.clone(),
            crate::lease::ToolKind::Observe,
            "live",
            "observe",
            serde_json::json!({"detail": "summary"}),
            async { Ok::<_, ToolError>(serde_json::json!({"mode":"live"})) },
        )
        .await;
```

Apply the same pattern (with `ToolKind::Mutate` for the error test that uses `clipboard_write`).

- [ ] **Step 7: Build + run tests.**

Run: `cargo build --workspace`
Expected: no errors.
Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 8: Commit.**

```bash
git add src/server.rs
git commit -m "feat(server): wire LeaseStore into KdeMcp + dispatch_audited per ADR-0011"
```

---

## Task 13: Remove `ModeLock` from `main.rs`; register release-on-exit

Drop the startup lock; let the lease handle coordination.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Remove the `ModeLock` import.**

In `src/main.rs`, change the import line from:

```rust
use kde_mcp::{KdeMcp, audit, cli::parse_and_validate, lock::ModeLock, mode, notify};
```

to:

```rust
use kde_mcp::{KdeMcp, audit, cli::parse_and_validate, mode, notify};
```

- [ ] **Step 2: Remove the `ModeLock::acquire(...)` block and the `notify_lock_acquired` call.**

Delete these lines from `main()`:

```rust
    let lock =
        ModeLock::acquire(mode_name).with_context(|| format!("acquiring {mode_name} mode lock"))?;
    tracing::info!(lock = %lock.path().display(), "mode lock acquired");

    notify::notify_lock_acquired(mode_name).await;
```

- [ ] **Step 3: Remove the trailing `drop(lock); notify::notify_lock_released(mode_name).await;` block.**

Replace those two lines with a call to release the lease on shutdown:

```rust
    server.lease.release_if_holder().ok();
    notify::notify_lease_released(mode_name).await;
```

(`notify_lease_released` is renamed in Task 14; for now this references a function that doesn't exist yet. Build will fail until then.)

- [ ] **Step 4: Build to confirm the expected breakage.**

Run: `cargo build`
Expected: error referencing `notify_lease_released` not found. That's the bridge to Task 14.

- [ ] **Step 5: Commit.**

Don't commit a broken tree. Hold the changes; complete Task 14 before committing both.

---

## Task 14: Notify lifecycle — `notify_lease_acquired` / `notify_lease_released`

Rename the existing functions and adjust their text to match ADR-0011's "first acquire from cold state → fire; 5s debounce on release". For v1, we keep the simple form (acquire on first call, release on shutdown) — the 5s debounce is a polish item, also captured here.

**Files:**
- Modify: `src/notify.rs`
- Modify: `src/main.rs`
- Modify: `src/lease.rs`

- [ ] **Step 1: In `src/notify.rs`, rename the two functions.**

Replace `pub async fn notify_lock_acquired(mode: &str)` with:

```rust
pub async fn notify_lease_acquired(mode: &str) {
    let mode_descriptor = match mode {
        "live" => "your live desktop",
        "virtual" => "a virtual desktop session",
        _ => "your desktop",
    };
    let body = format!("kde-mcp is controlling {mode_descriptor}. Press Meta+Esc to abort.");
    fire_blocking("Claude is using your computer", body, Urgency::Normal, 5000).await;
}
```

Replace `pub async fn notify_lock_released(mode: &str)` with:

```rust
pub async fn notify_lease_released(mode: &str) {
    let body = format!("kde-mcp finished using {mode} mode.");
    fire_blocking(
        "Claude finished using your computer",
        body,
        Urgency::Low,
        3000,
    )
    .await;
}
```

- [ ] **Step 2: Update the bottom-of-file no-op test.**

```rust
    #[test]
    fn module_compiles_and_exports_two_async_fns() {
        let _ = super::notify_lease_acquired;
        let _ = super::notify_lease_released;
    }
```

- [ ] **Step 3: Hook the first-acquire notification into the lease store.**

In `src/lease.rs`, add an `AtomicBool` to `LeaseStore` that tracks "have we ever fired the notification this process". Add it to the struct:

```rust
pub struct LeaseStore {
    config: LeaseConfig,
    self_pid: u32,
    self_session: String,
    notif_fired: std::sync::atomic::AtomicBool,
}
```

Update `LeaseStore::new`:

```rust
    pub fn new(config: LeaseConfig, self_pid: u32, self_session: String) -> Self {
        Self {
            config,
            self_pid,
            self_session,
            notif_fired: std::sync::atomic::AtomicBool::new(false),
        }
    }
```

At the end of the `Acquired` branch in `try_acquire_once` (right before `Ok(TryOutcome::Acquired)`), trigger the notify on the first-ever success:

```rust
        if !self
            .notif_fired
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            tokio::spawn(async { crate::notify::notify_lease_acquired("live").await });
        }
```

(The `"live"` is a placeholder; a follow-up can plumb the mode through `LeaseStore::new`. For now, the existing notify body says "your live desktop" — fine for the dominant case.)

- [ ] **Step 4: Update `src/main.rs` to call the renamed notify.**

The line from Task 13 step 3 already uses `notify_lease_released`. Make sure no `notify_lock_*` references remain. Run:

```sh
grep -n "notify_lock_" src/
```

Expected: no matches.

- [ ] **Step 5: Build + run tests.**

Run: `cargo build && cargo test --workspace`
Expected: clean build, all tests PASS.

- [ ] **Step 6: Commit both Tasks 13 and 14 together.**

```bash
git add src/main.rs src/notify.rs src/lease.rs
git commit -m "feat(lease): remove startup ModeLock; notify on first acquire / on release"
```

---

## Task 15: Delete `src/lock.rs` and `tests/integration/lockfile.rs`

`ModeLock` is now unreferenced. Remove the dead code.

**Files:**
- Delete: `src/lock.rs`
- Modify: `src/lib.rs`
- Delete: `tests/integration/lockfile.rs`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Confirm no remaining references to `ModeLock` or `lock::`.**

Run: `grep -rn "ModeLock\|crate::lock\|kde_mcp::lock" src/ tests/`
Expected: no matches.

- [ ] **Step 2: Remove `pub mod lock;` from `src/lib.rs`.**

Delete the line `pub mod lock;`.

- [ ] **Step 3: Delete `src/lock.rs`.**

```sh
rm src/lock.rs
```

- [ ] **Step 4: Remove `pub mod lockfile;` from `tests/integration.rs`.**

Delete the `pub mod lockfile;` line from the `mod integration { ... }` block.

- [ ] **Step 5: Delete `tests/integration/lockfile.rs`.**

```sh
rm tests/integration/lockfile.rs
```

- [ ] **Step 6: Build + run tests.**

Run: `cargo build --workspace && cargo test --workspace`
Expected: clean build, all tests PASS.

- [ ] **Step 7: Commit.**

```bash
git add src/lib.rs src/lock.rs tests/integration.rs tests/integration/lockfile.rs
git commit -m "chore(lock): remove dead ModeLock + lockfile tests (superseded by lease)"
```

---

## Task 16: Spec edits

ADR-0011's Consequences section names these edits. Apply them now so the spec matches the implementation.

**Files:**
- Modify: `docs/specs/2026-05-13-kde-computer-use-mcp-design.md`

- [ ] **Step 1: Replace §6 step 1 ("Mode lock check") with the lease check.**

Find the line beginning `1. **Mode lock check**` in §6.0. Replace it with:

```markdown
1. **Lease acquisition** — if the tool is mutating (per ADR-0011's classification table), the process must acquire-or-refresh the lease at `$XDG_RUNTIME_DIR/kde-mcp/lease.json`. Acquire blocks up to `acquire_timeout` (default 60s) polling at 100ms. Failure → `busy { holder_pid, holder_intent, lease_expires_in_ms, waited_ms, acquire_timeout_ms }`. Observing tools refresh the lease only when the caller already holds it; they never block.
```

- [ ] **Step 2: Replace the `lock_unavailable` row of §8's error table.**

Find the line beginning `| \`lock_unavailable\` |`. Replace with:

```markdown
| `busy` | another `kde-mcp` session currently holds the mutation lease (ADR-0011) | `holder_pid`, `holder_intent`, `lease_expires_in_ms`, `waited_ms`, `acquire_timeout_ms` |
```

- [ ] **Step 3: Replace §11.3 ("Single-session lock per mode") wholesale.**

Find the line beginning `3. **Single-session lock per mode.**`. Replace the entire paragraph with:

```markdown
3. **Per-session mutation lease.** `$XDG_RUNTIME_DIR/kde-mcp/lease.json` (POSIX flock on read/write). Multiple `kde-mcp --mode=live` may run concurrently — one per Claude Code session. Mutating tool calls serialize via the lease (acquire on dispatch, refresh on each subsequent call by the same holder, auto-expire after `idle_timeout` of inactivity, release on process exit / cancel). Observation calls are never blocked. Stale holders detected via `kill -0`. Notifications via `org.freedesktop.Notifications` on first acquire ("Claude is using your computer · press Meta+Esc to stop") and on clean shutdown ("Claude finished using your computer"). See `docs/adr/0011-multi-session-lease.md`.
```

- [ ] **Step 4: Verify the spec compiles as Markdown (no orphan references to `lock_unavailable`).**

Run: `grep -n "lock_unavailable" docs/specs/`
Expected: no matches.

- [ ] **Step 5: Commit.**

```bash
git add docs/specs/2026-05-13-kde-computer-use-mcp-design.md
git commit -m "docs(spec): update §6.0/§8/§11.3 for ADR-0011 lease semantics"
```

---

## Task 17: Runbook + SMOKE.md edits

The runbook's KNOWN-FIXES F3 entry and the SMOKE-test "Concurrent-launch contention" both reference the dead `lock_unavailable` code. Update them.

**Files:**
- Modify: `docs/runbook/KNOWN-FIXES.md`
- Modify: `tests/integration/SMOKE.md`

- [ ] **Step 1: Edit `docs/runbook/KNOWN-FIXES.md` F3 row.**

Replace the F3 row with:

```markdown
| F3 | `busy` | (none) | no | Another Claude Code session holds the mutation lease. **Log and retry the dependent step once** — the leaseholder's burst is usually short. Subsequent persistent `busy` is logged + the step is marked failed. |
```

- [ ] **Step 2: Edit `tests/integration/SMOKE.md` — replace the "Concurrent-launch contention" item.**

Find the bullet that starts with `- **Concurrent-launch contention** — start two \`--mode=live\` instances; …`. Replace the whole bullet (heading + code block) with:

```markdown
- **Concurrent-launch coexistence** — start two `--mode=live` instances; both should start successfully. Mutating tool calls from session B should block (returning `busy` after `acquire_timeout` if A holds for longer than the timeout):

  ```sh
  target/release/kde-mcp --mode=live &
  sleep 0.3
  target/release/kde-mcp --mode=live &   # expect: starts; both processes running
  sleep 0.3
  ls -la "$XDG_RUNTIME_DIR/kde-mcp/lease.json" 2>/dev/null || echo "(lease appears only when a mutating tool is in flight)"
  kill %1 %2 2>/dev/null
  wait %1 %2 2>/dev/null || true
  ```
```

- [ ] **Step 3: Update §A.4 of SMOKE.md ("What pass means") to drop the lockfile bullet.**

Find the bullet beginning `- holds the per-mode lockfile, writes the holder PID, releases on exit,` and replace with:

```markdown
- starts cleanly with no startup lock (per ADR-0011); mutation lease is per-call, opened on demand,
```

- [ ] **Step 4: Confirm no lingering `lock_unavailable` references in docs.**

Run: `grep -rn "lock_unavailable" docs/ tests/`
Expected: no matches.

- [ ] **Step 5: Commit.**

```bash
git add docs/runbook/KNOWN-FIXES.md tests/integration/SMOKE.md
git commit -m "docs(runbook): replace lock_unavailable / concurrent-contention with ADR-0011 lease semantics"
```

---

## Task 18: End-to-end verification

A green workspace test run and a clippy pass before signing off.

**Files:** none.

- [ ] **Step 1: Format + lint.**

Run: `cargo fmt --all`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no output / no warnings.

- [ ] **Step 2: Full test suite.**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: four `test result: ok. … 0 failed` lines; total ≥ 115 (was 110; lease added ~8 tests, lockfile removed 4).

- [ ] **Step 3: Confirm wire-tool count still 16.**

Run: `cargo test --test integration integration::tools_list::server_advertises_sixteen_tools_with_expected_names -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Manual two-session smoke (only if iterating live).**

Open two terminals. In each, start the release binary directly so we can observe behaviour without rmcp wrapping:

```sh
cargo build --release
# Terminal 1:
target/release/kde-mcp --mode=live
# Terminal 2 (a second later):
target/release/kde-mcp --mode=live
```

Expected: both start without errors (no `Error: lock unavailable`).

End the smoke by sending each `Ctrl-D` (EOF on stdin).

- [ ] **Step 5: Commit any fmt-induced touch-ups.**

```bash
git status
# If anything changed:
git add -u
git commit -m "chore: cargo fmt after lease implementation"
```

---

## Out-of-scope notes (carry to follow-up)

- **KGlobalAccel Meta+Esc handoff** between leaseholders (ADR-0011 §"Meta+Esc handoff"). Current state: every kde-mcp process registers on startup. KGlobalAccel's `doRegister` is documented as idempotent for an already-registered component; with overlapping registrations, Meta+Esc fires the cancel signal in all listening sessions. This means the hotkey effectively cancels all sessions at once — usable as a "panic button" but not the per-leaseholder handoff the ADR specifies. Carry to a follow-up task once the lease is shipped and observed in live use.
- **Per-mode `lease.json`.** Right now there's one lease file shared by `--mode=live` and `--mode=virtual`. The original lock split them; the lease doesn't yet. If concurrent live+virtual sessions become common, split via `lease-{mode}.json` paths — straightforward extension, no design change.
- **`policy.toml` `[lease]` section.** ADR-0011 mentions the config knob; this plan keeps `LeaseConfig::default()` hardcoded. Add a tiny TOML loader in a follow-up — model on the existing `PolicyConfig::load_default`.
