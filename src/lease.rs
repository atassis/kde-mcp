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
        let mut guard = lock.write()?;
        guard.seek(SeekFrom::Start(0))?;
        let mut body = String::new();
        guard.read_to_string(&mut body)?;
        let is_ours = serde_json::from_str::<LeaseRecord>(&body)
            .map(|r| r.holder_pid == self.self_pid)
            .unwrap_or(false);
        // Drop the guard before unlinking.
        drop(guard);
        if is_ours {
            // Best-effort: ignore ENOENT if another process raced us.
            let _ = std::fs::remove_file(&path);
        }
        Ok(())
    }
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
