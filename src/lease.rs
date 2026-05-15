//! Per-session mutation lease (ADR-0011).
//!
//! Replaces the startup `<mode>.lock` (spec §11.3) with workflow-level
//! mutual exclusion. Mutating tools acquire the lease at dispatch; observing
//! tools refresh it only if the caller already holds it. The lease lives at
//! `$XDG_RUNTIME_DIR/kde-mcp/lease.json` and is guarded by POSIX flock.
//!
//! See `docs/adr/0011-multi-session-lease.md` for the design rationale.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct LeaseConfig {
    /// How long after the holder's most recent call the lease stays held
    /// before it idle-expires. Default 10s (ADR-0011).
    pub idle_timeout: Duration,
    /// How long an acquire call blocks waiting for the lease before
    /// returning `busy`. Default 60s (ADR-0011).
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub holder_pid: u32,
    pub holder_session_id: String,
    pub holder_intent: String,
    pub acquired_at: DateTime<Utc>,
    pub last_refresh_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub fn lease_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("kde-mcp").join("lease.json")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_alive_true_for_self_false_for_zero() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }
}
