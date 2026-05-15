//! Per-MCP-session attribution for daemon mode (ADR-0012 §7).
//!
//! Each new HTTP connection (specifically, each rmcp `initialize` call)
//! mints a fresh [`SessionId`] (UUID v4). The id threads through:
//!
//! - The ADR-0011 lease record (already keyed by `holder_session_id` —
//!   no schema change needed).
//! - The audit log (new optional `session_id` field — see Phase 5
//!   audit-record changes).
//! - The per-session cancel-token registry, so the tray's
//!   "Abort current session" item can fire just that session's tokens
//!   without disturbing others (Phase 7 wires the tray side).
//!
//! `SessionRegistry` is a thin wrapper around `DashMap` so it's cheaply
//! cloneable and safe to share across the rmcp factory closure, the
//! tray, and the cancel layer.

use dashmap::DashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A daemon-minted MCP session identifier. Wraps `Uuid` so the type can
/// not be confused with the bus session id, the audit session id, or
/// anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        SessionId(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SessionId(Uuid::parse_str(s)?))
    }
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub cancel: CancellationToken,
}

/// Concurrent map of `SessionId -> SessionEntry`. Cheaply cloneable.
#[derive(Default, Clone)]
pub struct SessionRegistry {
    inner: Arc<DashMap<SessionId, SessionEntry>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh session_id with its own cancellation token; return
    /// both. The caller should keep the token to wire it into the
    /// per-session [`KdeMcp`] state and to fire it on disconnect.
    pub fn open(&self) -> (SessionId, CancellationToken) {
        let id = SessionId::new();
        let cancel = CancellationToken::new();
        self.inner.insert(
            id,
            SessionEntry {
                cancel: cancel.clone(),
            },
        );
        (id, cancel)
    }

    /// Drop the session state. The cancel token's downstream subscribers
    /// still observe their previously-cloned handles — closing here only
    /// removes the registry entry, not the token itself.
    pub fn close(&self, id: SessionId) {
        self.inner.remove(&id);
    }

    pub fn count(&self) -> usize {
        self.inner.len()
    }

    /// Fire only the named session's cancel token. Returns true if the
    /// id was present.
    pub fn cancel(&self, id: SessionId) -> bool {
        if let Some(entry) = self.inner.get(&id) {
            entry.cancel.cancel();
            true
        } else {
            false
        }
    }

    /// Fire every session's cancel token (used by tray "Abort all" and
    /// by Meta+Esc once Phase 7 wires it).
    pub fn cancel_all(&self) {
        for entry in self.inner.iter() {
            entry.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_mints_unique_ids() {
        let r = SessionRegistry::new();
        let (a, _) = r.open();
        let (b, _) = r.open();
        assert_ne!(a, b);
        assert_eq!(r.count(), 2);
        r.close(a);
        assert_eq!(r.count(), 1);
    }

    #[test]
    fn session_id_round_trips_via_string() {
        let id = SessionId::new();
        let s = id.to_string();
        let back: SessionId = s.parse().expect("parse");
        assert_eq!(id, back);
    }

    #[test]
    fn cancel_only_target_session() {
        let r = SessionRegistry::new();
        let (a, ca) = r.open();
        let (_b, cb) = r.open();
        assert!(r.cancel(a));
        assert!(ca.is_cancelled());
        assert!(!cb.is_cancelled());
    }

    #[test]
    fn cancel_all_fires_every_token() {
        let r = SessionRegistry::new();
        let (_, ca) = r.open();
        let (_, cb) = r.open();
        r.cancel_all();
        assert!(ca.is_cancelled());
        assert!(cb.is_cancelled());
    }

    #[test]
    fn cancel_missing_id_returns_false() {
        let r = SessionRegistry::new();
        let bogus = SessionId::new();
        assert!(!r.cancel(bogus));
    }
}
