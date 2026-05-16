//! OAuth 2.1 authorization for the daemon's MCP resource (ADR-0014).
//!
//! The daemon acts as both the protected resource (`/mcp`) and its own
//! authorization server. Tokens are minted on demand by `/oauth/token`,
//! held in an in-memory set with a TTL, and validated by
//! [`oauth_validate_middleware`] on each `/mcp` request.
//!
//! ## Wire surface
//!
//! - `GET /.well-known/oauth-protected-resource` — RFC 9728 metadata.
//! - `GET /.well-known/oauth-authorization-server` — RFC 8414 metadata.
//! - `POST /oauth/register` — RFC 7591 Dynamic Client Registration.
//!   Unguarded; returns an opaque `client_id`. We don't track ids.
//! - `POST /oauth/token` — RFC 6749 §4.4 client_credentials. Unguarded;
//!   any caller from the bound interface gets a fresh bearer.
//! - `POST /mcp` — protected; 401 challenge mounts a proper RFC 6750 §3
//!   envelope so OAuth clients can discover the auth server.
//!
//! ## Trust boundary
//!
//! The bind address (`127.0.0.1` for `serve_http`) is the trust gate.
//! Same-UID different-process attackers can hit `/oauth/token`; this
//! matches the bearer-file world's same-UID exposure. The hardening
//! beyond that is `--transport=unix` (see [`crate::transport::daemon`]).
//!
//! ## Lifetime
//!
//! Tokens live in `HashMap<String, Instant>`. A background sweeper
//! ([`spawn_sweeper`]) prunes expired entries every
//! [`SWEEP_INTERVAL`]. Daemon restart clears the map → clients re-auth
//! transparently via their OAuth abstraction.

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use parking_lot::RwLock;
use rand::TryRngCore;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default token TTL. 1 hour: short enough to bound a stolen-token's
/// blast radius; long enough to amortise the OAuth round-trip across
/// hundreds of MCP calls.
pub const TOKEN_TTL: Duration = Duration::from_secs(3600);

/// How often the background sweeper prunes expired tokens.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Token entropy in bytes (32 → 64 hex chars).
const TOKEN_BYTES: usize = 32;

/// In-memory issued-tokens store, plus the issuer URL the well-known
/// metadata advertises. Cloned cheaply.
#[derive(Clone)]
pub struct OAuthState {
    inner: Arc<RwLock<HashMap<String, Instant>>>,
    issuer: String,
}

impl OAuthState {
    /// `issuer` is the public base URL the well-known endpoints
    /// advertise (e.g. `http://127.0.0.1:7575`). Must match what the
    /// client uses to dial us, otherwise the metadata redirects fail.
    pub fn new(issuer: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            issuer,
        }
    }

    /// Mint a fresh 32-byte hex token, record its expiry, return it.
    pub fn issue(&self) -> anyhow::Result<String> {
        let mut bytes = [0u8; TOKEN_BYTES];
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|e| anyhow::anyhow!("OsRng::try_fill_bytes: {e}"))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let expiry = Instant::now() + TOKEN_TTL;
        self.inner.write().insert(hex.clone(), expiry);
        Ok(hex)
    }

    /// `true` iff the token was issued by us and hasn't expired yet.
    pub fn validate(&self, token: &str) -> bool {
        if let Some(&expiry) = self.inner.read().get(token) {
            expiry > Instant::now()
        } else {
            false
        }
    }

    /// Prune expired entries. Takes the write lock only if there's
    /// something to drop.
    pub fn sweep(&self) {
        let now = Instant::now();
        let stale: Vec<String> = {
            let guard = self.inner.read();
            guard
                .iter()
                .filter_map(|(k, &v)| if v <= now { Some(k.clone()) } else { None })
                .collect()
        };
        if stale.is_empty() {
            return;
        }
        let mut guard = self.inner.write();
        for k in stale {
            guard.remove(&k);
        }
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Test-only: insert a token with an explicit expiry instant.
    #[cfg(test)]
    pub fn insert_for_test(&self, token: String, expiry: Instant) {
        self.inner.write().insert(token, expiry);
    }
}

/// Build the unauthenticated discovery + token routes. Mount these on
/// the router **outside** the `/mcp`-protecting middleware — they MUST
/// be reachable without a bearer, otherwise no client can ever
/// bootstrap.
pub fn router(state: OAuthState) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/oauth/register", post(register_client))
        .route("/oauth/token", post(token_endpoint))
        .with_state(state)
}

/// Spawn a background task that calls [`OAuthState::sweep`] every
/// [`SWEEP_INTERVAL`] until `cancel` fires.
pub fn spawn_sweeper(state: OAuthState, cancel: tokio_util::sync::CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(SWEEP_INTERVAL) => state.sweep(),
                _ = cancel.cancelled() => return,
            }
        }
    });
}

async fn protected_resource_metadata(State(state): State<OAuthState>) -> Json<serde_json::Value> {
    Json(json!({
        "resource": format!("{}/mcp", state.issuer),
        "authorization_servers": [&state.issuer],
        "bearer_methods_supported": ["header"],
    }))
}

async fn authorization_server_metadata(State(state): State<OAuthState>) -> Json<serde_json::Value> {
    Json(json!({
        "issuer": &state.issuer,
        "token_endpoint": format!("{}/oauth/token", state.issuer),
        "registration_endpoint": format!("{}/oauth/register", state.issuer),
        "grant_types_supported": ["client_credentials"],
        "token_endpoint_auth_methods_supported": ["none"],
        "response_types_supported": ["token"],
    }))
}

/// Open registration. We mint an opaque `client_id` and don't track it
/// — the trust signal is "request came in on the bound interface", not
/// "client is in our registry."
async fn register_client() -> (StatusCode, Json<serde_json::Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": id,
            "client_secret_expires_at": 0,
            "client_id_issued_at": now,
            "token_endpoint_auth_method": "none",
            "grant_types": ["client_credentials"],
            "response_types": ["token"],
        })),
    )
}

/// `POST /oauth/token` — client_credentials grant. Body is ignored; we
/// always issue. Returns the standard OAuth 2.0 token-response shape.
async fn token_endpoint(
    State(state): State<OAuthState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let token = state.issue().map_err(|e| {
        tracing::error!("oauth: token issue failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({
        "access_token": token,
        "token_type": "Bearer",
        "expires_in": TOKEN_TTL.as_secs(),
    })))
}

/// Middleware that gates `/mcp`. Unknown / missing / expired tokens get
/// a proper RFC 6750 §3 401 — header + JSON body — so OAuth clients
/// (Claude Code 2.1.139, etc.) can drive the discovery flow.
pub async fn oauth_validate_middleware(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    req: Request<Body>,
    next: Next,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) if state.validate(t) => next.run(req).await,
        Some(_) => {
            tracing::warn!("daemon: 401 (stale or unknown Bearer)");
            challenge(&state, "invalid_token", "token unknown or expired")
        }
        None => {
            tracing::warn!("daemon: 401 (missing Authorization)");
            challenge(
                &state,
                "unauthorized",
                "kde-mcp requires an OAuth 2.1 bearer; see WWW-Authenticate",
            )
        }
    }
}

fn challenge(state: &OAuthState, error: &str, description: &str) -> Response {
    let metadata_url = format!("{}/.well-known/oauth-protected-resource", state.issuer);
    let mut res = (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": error,
            "error_description": description,
        })),
    )
        .into_response();
    let www = format!(r#"Bearer realm="kde-mcp", resource_metadata="{metadata_url}""#);
    if let Ok(v) = HeaderValue::from_str(&www) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_returns_64_hex_chars() {
        let s = OAuthState::new("http://localhost".into());
        let t = s.issue().unwrap();
        assert_eq!(t.len(), 64, "32 bytes = 64 hex chars");
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn validate_accepts_just_issued_rejects_unknown() {
        let s = OAuthState::new("http://localhost".into());
        let t = s.issue().unwrap();
        assert!(s.validate(&t));
        assert!(!s.validate("deadbeef"));
        assert!(!s.validate(""));
    }

    #[test]
    fn sweep_removes_expired_keeps_live() {
        let s = OAuthState::new("http://localhost".into());
        let live = s.issue().unwrap();
        s.insert_for_test("stale".into(), Instant::now() - Duration::from_secs(1));
        assert!(s.validate(&live));
        // `validate` already filters by expiry; sweep is for memory hygiene.
        s.sweep();
        assert!(s.validate(&live));
        assert!(!s.validate("stale"));
    }

    #[test]
    fn two_issues_yield_distinct_tokens() {
        let s = OAuthState::new("http://localhost".into());
        let a = s.issue().unwrap();
        let b = s.issue().unwrap();
        assert_ne!(a, b);
    }
}
