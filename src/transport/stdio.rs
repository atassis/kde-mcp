//! Stdio MCP transport — original one-client-per-process design.
//!
//! Reads MCP frames on stdin, writes responses on stdout. Exits cleanly
//! when the parent closes stdin (i.e., when Claude Code terminates the
//! session). Releases the per-session lease and fires the
//! "Claude finished using your computer" notification on shutdown.

use crate::server::KdeMcp;
use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};

pub async fn serve(server: KdeMcp) -> Result<()> {
    let lease = server.lease.clone();
    let mode_name = server.mode.as_str();

    let service = server
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("rmcp serve: {e}"))?;

    service.waiting().await?;

    lease.release_if_holder().ok();
    crate::notify::notify_lease_released(mode_name).await;
    tracing::info!("shut down cleanly");
    Ok(())
}
