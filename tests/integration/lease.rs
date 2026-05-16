//! Integration tests for the multi-session lease (ADR-0011).
//!
//! Each test sets `XDG_RUNTIME_DIR` to a fresh tempdir so concurrent test
//! runs don't share lease files. Because `XDG_RUNTIME_DIR` is process-wide,
//! tests that mutate it must serialize via `ENV_LOCK`.

use chrono::{TimeZone, Utc};
use kde_mcp::lease::{
    AcquireError, LeaseConfig, LeaseRecord, LeaseStore, ToolKind, TryOutcome, classify_tool,
    lease_path,
};
use std::sync::Mutex;
use std::time::Duration;

/// Serialise all tests that touch `XDG_RUNTIME_DIR` so they don't race each
/// other on the process-wide env var.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn default_config_matches_adr_0011() {
    let c = LeaseConfig::default();
    assert_eq!(c.idle_timeout, Duration::from_secs(10));
    assert_eq!(c.acquire_timeout, Duration::from_secs(60));
}

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
    assert!(
        s.contains("\"holder_pid\""),
        "on-disk key must be holder_pid per ADR-0011: {s}"
    );
    assert!(
        s.contains("\"last_refresh_at\""),
        "on-disk key must be last_refresh_at per ADR-0011: {s}"
    );
    let parsed: LeaseRecord = serde_json::from_str(&s).unwrap();
    assert_eq!(parsed, r);
}

#[test]
fn lease_path_lives_under_xdg_runtime_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let p = lease_path();
    assert!(p.starts_with(dir.path()));
    assert!(p.ends_with("kde-mcp/lease.json"));
}

#[test]
fn try_acquire_on_empty_file_succeeds() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    // Snapshot the expected path while we still own XDG_RUNTIME_DIR.
    let expected_path = lease_path();
    let store = LeaseStore::new(
        LeaseConfig::default(),
        std::process::id(),
        "session-A".into(),
    );
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, TryOutcome::Acquired));
    assert!(
        expected_path.exists(),
        "lease file should exist after acquire"
    );
}

#[test]
fn try_acquire_when_alive_other_holder_returns_busy() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let expected_path = lease_path();
    // Write a lease record manually pointing at PID 1 (init — always alive on Linux).
    let rec = LeaseRecord {
        holder_pid: 1,
        holder_session_id: "alien".into(),
        holder_intent: "mouse".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
    std::fs::write(&expected_path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store = LeaseStore::new(LeaseConfig::default(), 99999, "session-B".into());
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    match outcome {
        TryOutcome::Busy(holder) => {
            assert_eq!(holder.holder_pid, 1);
            assert_eq!(holder.holder_intent, "mouse");
        }
        TryOutcome::Acquired => panic!("expected Busy, got Acquired"),
    }
}

#[test]
fn try_acquire_when_dead_holder_takes_lease() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let expected_path = lease_path();
    // PID 0xFFFFFFE is far above kernel.pid_max (default 4194304 on Linux) —
    // the kill(0) probe will return ESRCH, so try_acquire_once treats the
    // lease as stale.
    let rec = LeaseRecord {
        holder_pid: 0xFFFFFFE,
        holder_session_id: "ghost".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: chrono::Utc::now(),
        last_refresh_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
    };
    std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
    std::fs::write(&expected_path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, TryOutcome::Acquired));
}

#[test]
fn try_acquire_when_expired_lease_takes_it() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let expected_path = lease_path();
    let rec = LeaseRecord {
        holder_pid: 1, // init, alive
        holder_session_id: "expired".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: chrono::Utc::now() - chrono::Duration::seconds(60),
        last_refresh_at: chrono::Utc::now() - chrono::Duration::seconds(30),
        expires_at: chrono::Utc::now() - chrono::Duration::seconds(20),
    };
    std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
    std::fs::write(&expected_path, serde_json::to_string(&rec).unwrap()).unwrap();

    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let outcome = store.try_acquire_once("keyboard.type").unwrap();
    assert!(matches!(outcome, TryOutcome::Acquired));
}

#[test]
fn refresh_if_holder_is_no_op_when_not_holder() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = LeaseRecord {
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
    let body = std::fs::read_to_string(&path).unwrap();
    let after: LeaseRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(after, before);
}

#[test]
fn refresh_if_holder_extends_expires_at() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let _ = store.try_acquire_once("act_on_element").unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    let before: LeaseRecord = serde_json::from_str(&body).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(store.refresh_if_holder().unwrap());
    let body = std::fs::read_to_string(&path).unwrap();
    let after: LeaseRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(after.holder_pid, before.holder_pid);
    assert_eq!(after.acquired_at, before.acquired_at);
    assert!(after.last_refresh_at >= before.last_refresh_at);
    assert!(after.expires_at >= before.expires_at);
}

#[test]
fn release_if_holder_removes_lease_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let _ = store.try_acquire_once("clipboard_write").unwrap();
    assert!(path.exists());
    store.release_if_holder().unwrap();
    assert!(!path.exists(), "lease file should be deleted on release");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn acquire_returns_busy_after_short_timeout_when_held() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let path = lease_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rec = LeaseRecord {
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

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn acquire_returns_immediately_when_no_holder() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let store = LeaseStore::new(LeaseConfig::default(), std::process::id(), "us".into());
    let started = std::time::Instant::now();
    store.acquire("keyboard.type").await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
}

#[test]
fn observe_tools_classified_as_observe() {
    for name in [
        "observe",
        "inspect",
        "screenshot",
        "read_text",
        "window_query",
        "clipboard_read",
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
    ] {
        assert_eq!(
            classify_tool(name, &serde_json::Value::Null),
            ToolKind::Mutate,
            "expected {name} to be Mutate"
        );
    }
}
