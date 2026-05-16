//! Wire-transport dispatch (ADR-0012).
//!
//! `Transport::Stdio` keeps the original one-client-per-process semantics
//! from before ADR-0012 — Claude Code spawns a kde-mcp child and speaks
//! over stdin/stdout. `Transport::Daemon` is the singleton long-running
//! HTTP server on `127.0.0.1:7575`, activated by systemd-user. Both modes
//! are first-class peers and can run in parallel against the same on-disk
//! state (lease, approvals, audit log).

pub use crate::cli::Transport;

pub mod daemon;
pub mod oauth;
pub mod session;
pub mod stdio;
