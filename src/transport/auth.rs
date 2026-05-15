//! Bearer-token authentication for the daemon's `/mcp` endpoint
//! (ADR-0012 §6).
//!
//! The token lives at `$XDG_RUNTIME_DIR/kde-mcp/token` with mode 0600.
//! Generated on first daemon launch (32 random bytes, hex-encoded);
//! survives daemon restarts within a login session (tmpfs-backed); is
//! deleted when the user logs out (the runtime dir goes away).
//!
//! `/healthz` deliberately stays unauthenticated — it returns no
//! sensitive data and is useful for install-time sanity checks.

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use rand::TryRngCore;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Path of the bearer-token file. On Plasma this is typically
/// `/run/user/<uid>/kde-mcp/token`.
pub fn default_token_path() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(d).join("kde-mcp/token");
    }
    PathBuf::from("/tmp/kde-mcp-token")
}

/// Read the token at `path`. If the file is missing or empty, generate
/// a fresh 32-byte hex token, write it with mode 0600 (creating parent
/// dirs as needed), and return it.
pub fn ensure_token(path: &Path) -> Result<String> {
    if let Ok(existing) = fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .context("OsRng::try_fill_bytes")?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::write(path, &hex).with_context(|| format!("write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))?;
    Ok(hex)
}

/// Per-instance state for the bearer-auth middleware. Cloned cheaply
/// (String is moved into Arc by axum's State machinery).
#[derive(Clone)]
pub struct AuthState {
    pub token: String,
}

/// axum middleware that rejects requests whose `Authorization` header
/// does not match `Bearer <state.token>`. Attach via
/// `from_fn_with_state(state, bearer_middleware)`.
pub async fn bearer_middleware(
    State(state): State<AuthState>,
    headers: HeaderMap,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let want = format!("Bearer {}", state.token);
    let got = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if got == want {
        Ok(next.run(req).await)
    } else {
        tracing::warn!(
            "daemon: 401 ({} Authorization)",
            if got.is_empty() { "missing" } else { "wrong" }
        );
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_token_creates_file_with_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("token");
        let tok = ensure_token(&path).expect("write");
        assert_eq!(tok.len(), 64, "32 bytes = 64 hex chars");
        assert!(tok.chars().all(|c| c.is_ascii_hexdigit()));
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn ensure_token_returns_existing_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("token");
        let first = ensure_token(&path).expect("create");
        let second = ensure_token(&path).expect("read");
        assert_eq!(first, second);
    }

    #[test]
    fn ensure_token_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/sub/token");
        let _ = ensure_token(&path).expect("write with parents");
        assert!(path.exists());
    }

    #[test]
    fn ensure_token_replaces_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("token");
        std::fs::write(&path, "").unwrap();
        let tok = ensure_token(&path).expect("regen");
        assert_eq!(tok.len(), 64);
    }
}
