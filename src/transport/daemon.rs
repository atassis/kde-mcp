//! Daemon (HTTP) MCP transport — singleton per user (ADR-0012).
//!
//! Phase 1: stub only. Returns a structured error so callers know the
//! mode exists but isn't yet implemented. Phase 2 lands the axum server
//! + `/healthz` route; subsequent phases mount rmcp, add auth, session
//! attribution, idle stop, tray integration.

use crate::server::KdeMcp;
use anyhow::{Result, anyhow};

pub async fn serve(_server: KdeMcp) -> Result<()> {
    Err(anyhow!(
        "daemon mode not yet implemented (see docs/plans/2026-05-16-singleton-daemon-implementation.md phase 2+)"
    ))
}
