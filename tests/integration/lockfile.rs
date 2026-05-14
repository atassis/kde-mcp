use kde_mcp::lock::{LockError, ModeLock};

// Environment mutation is process-global. We use unique mode names per
// test to dodge interference, but XDG_RUNTIME_DIR is set in each test.
// Cargo runs tests in parallel by default; the test binary will set the
// env var, but the actual lockfile path is derived freshly per call so
// concurrent reads are fine. Writes within ModeLock::acquire are guarded
// by the OS-level flock itself.

fn set_runtime_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    dir
}

#[test]
fn second_acquire_in_same_process_fails() {
    let _g = set_runtime_dir();
    let _a = ModeLock::acquire("live-test-a").unwrap();
    let err = ModeLock::acquire("live-test-a").unwrap_err();
    assert!(matches!(err, LockError::AlreadyHeld { .. }));
    if let LockError::AlreadyHeld { pid } = err {
        assert_eq!(pid, std::process::id(), "stored pid should be ours");
    }
}

#[test]
fn different_modes_can_coexist() {
    let _g = set_runtime_dir();
    let _live = ModeLock::acquire("live-test-b").unwrap();
    let _virt = ModeLock::acquire("virtual-test-b").unwrap();
}

#[test]
fn release_on_drop_allows_reacquire() {
    let _g = set_runtime_dir();
    {
        let _a = ModeLock::acquire("live-test-c").unwrap();
    }
    let _b = ModeLock::acquire("live-test-c").unwrap();
}

#[test]
fn lockfile_contains_holder_pid() {
    let dir = set_runtime_dir();
    let lock = ModeLock::acquire("live-test-d").unwrap();
    let body = std::fs::read_to_string(lock.path()).unwrap();
    let pid: u32 = body.trim().parse().unwrap();
    assert_eq!(pid, std::process::id());
    let _ = dir; // keep tempdir alive
}
