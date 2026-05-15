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
