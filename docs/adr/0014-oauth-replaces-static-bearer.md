# ADR-0014: Daemon is the OAuth 2.1 authorization server for its own MCP resource

**Status:** Superseded by [[0015-remove-oauth-rely-on-bind]] (2026-05-18). The OAuth layer was protocol theater for our threat model — Claude Code 2.1.139 happily connects to an unauthenticated `/mcp` when the server doesn't advertise OAuth discovery.

*Originally accepted 2026-05-16.*

Supersedes the static-bearer-token mechanism from [[0012-singleton-daemon-http]] §4 ("bearer-token auth on /mcp"). The HTTP transport, the singleton-daemon shape, the systemd socket-activation, and the lease semantics are unchanged. Only the **authorization layer** flips: from a long-lived file-synced shared secret to a short-lived OAuth-2.1 access token negotiated at MCP-connect time.

## Context

ADR-0012 §4 chose "bearer token at `$XDG_RUNTIME_DIR/kde-mcp/token`, written into `~/.claude.json`'s `Authorization` header by `packaging/install.sh`." That choice was defensible at the time — MCP HTTP clients in late 2025 had not standardised on OAuth yet, and a single shared secret read from a 0600 file was the obvious workaround for "TCP loopback has no kernel-level peer-credential check."

A year on, both the protocol and the client have moved:

1. **MCP spec 2025-11-25 mandates OAuth 2.1** (RFC 6749 / 8414 / 9728) for HTTP transports. Servers must serve `/.well-known/oauth-protected-resource`. Clients are expected to do the OAuth dance at MCP-connect time. The static-bearer pattern is non-conformant for any spec ≥ 2025-06-18.

2. **Claude Code 2.1.139 already drives that flow.** Per `~/.cache/claude-cli-nodejs/<repo>/mcp-logs-kde-mcp/2026-05-16T08-38-58-356Z.jsonl`:
   ```
   "HTTP transport options": { ..., "hasAuthProvider": true }
   "HTTP 401: Invalid OAuth error response: SyntaxError: JSON Parse error: Unexpected EOF. Raw body: "
   ```
   The client expected an OAuth challenge in the 401 body and got our empty axum response. The current bearer middleware (`src/transport/auth.rs::bearer_middleware`) returns a bare 401 with no `WWW-Authenticate`, no metadata URL, no error envelope. That's why even a **matching** bearer would have to go through Claude's OAuth abstraction — and ours doesn't speak the language.

3. **The token-in-tmpfs hack is leaking gotchas.** As verified empirically on 2026-05-16: `$XDG_RUNTIME_DIR` lives in tmpfs (`/run/user/$UID`), so it gets wiped on logout / reboot / systemd-tmpfiles sweep. The daemon re-mints on next start (idempotent if the file is present; new token if not). `~/.claude.json` retains the *old* bearer because nothing rewrites it. The MCP connection breaks until the user re-runs `packaging/install.sh --transport=http`. We chased patching this — move the file to disk; have the daemon edit `~/.claude.json` on startup — and every patch is downstream of the real misuse: **a static shared secret is the wrong primitive for local IPC authorization**.

4. **What "local IPC" actually wants.** The threat model is "another process on this machine connecting to `127.0.0.1:7575`." On a single-user workstation:
   - Same-UID malicious process: can read the bearer file regardless. Defeated.
   - Different-UID local process: the kernel's `SO_PEERCRED` on a Unix socket is the right check. We already implement `--transport=unix` (`kde-mcp.socket`); we're blocked on `unix://` URL support in Claude Code's HTTP MCP client. That's the long-term fix.
   - External-machine attacker: defeated by binding to `127.0.0.1` only — bearer adds nothing.

   Until the upstream `unix://` lands, OAuth 2.1 with short-lived tokens is the protocol-conformant intermediate.

## Decision

**Implement OAuth 2.1 authorization in the daemon, replacing the static-bearer middleware. `~/.claude.json` stops carrying any `Authorization` header.**

### Wire-level behaviour

1. **Unauth'd request to `/mcp` →** `401 Unauthorized` with:
   - Header: `WWW-Authenticate: Bearer realm="kde-mcp", resource_metadata="http://127.0.0.1:7575/.well-known/oauth-protected-resource"`
   - Body: `{"error": "unauthorized", "error_description": "kde-mcp requires OAuth 2.1 bearer; see WWW-Authenticate"}` (RFC 6750 §3 envelope shape — empties trip clients like Claude Code 2.1.139).

2. **`GET /.well-known/oauth-protected-resource` →** `200` JSON (RFC 9728):
   ```json
   {
     "resource": "http://127.0.0.1:7575/mcp",
     "authorization_servers": ["http://127.0.0.1:7575"],
     "bearer_methods_supported": ["header"]
   }
   ```

3. **`GET /.well-known/oauth-authorization-server` →** `200` JSON (RFC 8414):
   ```json
   {
     "issuer": "http://127.0.0.1:7575",
     "token_endpoint": "http://127.0.0.1:7575/oauth/token",
     "registration_endpoint": "http://127.0.0.1:7575/oauth/register",
     "grant_types_supported": ["client_credentials"],
     "token_endpoint_auth_methods_supported": ["none"],
     "response_types_supported": ["token"]
   }
   ```

4. **`POST /oauth/register` (Dynamic Client Registration, RFC 7591) →** `201` with an arbitrary `client_id`. We do not gate registration — any caller from 127.0.0.1 can register. Returned `client_id` is opaque; we don't track them anywhere persistent.

5. **`POST /oauth/token` (RFC 6749 §4.4 client_credentials grant) →** `200` with:
   ```json
   {"access_token": "<32-byte hex>", "token_type": "Bearer", "expires_in": 3600}
   ```
   No client authentication required (we accept `token_endpoint_auth_methods_supported: ["none"]` per RFC 8414 §2). Trust boundary: the request came in on `127.0.0.1`. Daemon adds the token to an in-memory `HashSet<String>` with a per-entry TTL.

6. **`POST /mcp` with `Authorization: Bearer <issued-token>` →** validated against the in-memory set. Unknown / expired token → 401 with the same challenge as step 1. Claude Code's OAuth client automatically refreshes.

### Code shape

- New file `src/transport/oauth.rs` (~250 LOC) — endpoints + the validator middleware + a `tokio::time::interval`-driven sweep that prunes expired entries.
- `src/transport/auth.rs` deleted (token-file logic + bearer middleware retire).
- `src/transport/daemon.rs::http_router` swaps the middleware: `from_fn_with_state(auth_state, bearer_middleware)` → `from_fn_with_state(oauth_state, oauth_validate_middleware)`. The well-known + `/oauth/*` routes are added before the middleware (they must be reachable unauthenticated).
- `packaging/install.sh` simplifies: the `case http) ... TOKEN=... --header "Authorization: Bearer ..."` block goes away. The MCP-add call becomes `claude mcp add --transport http --scope user kde-mcp http://127.0.0.1:7575/mcp` — no headers.
- `Cargo.toml`: no new deps (axum + serde + rand + uuid cover this).
- Integration tests (`tests/integration/oauth.rs`): the 401 challenge shape, the well-known JSON, the token-endpoint round-trip, a stale-token rejection.

### Token lifetime + persistence

- **In-memory only.** No disk file. Daemon restart = all issued tokens invalid. Clients re-auth automatically.
- **TTL 1 hour** by default. Short enough that a stolen token's blast radius is bounded; long enough that we don't pay an OAuth round-trip per MCP call.
- **No refresh tokens.** The token endpoint is open to anything on `127.0.0.1`; clients re-request rather than refresh. Simpler.

### Threat-model unchanged from ADR-0012

- Same-UID malicious process: defeated only by Unix-socket peer-cred. OAuth is no better than bearer here; both lose. The `--transport=unix` path is the eventual fix once Claude Code supports it.
- Different-UID local process: defeated by `127.0.0.1`-only bind + the fact that the token endpoint is *also* `127.0.0.1`-only. A different-UID local process CAN hit `/oauth/token` and get a token (loopback is permissive across UIDs on Linux). Identical to the bearer-file world, where they could `cat $XDG_RUNTIME_DIR/kde-mcp/token` (...except they can't due to 0600 — so bearer was strictly stronger here). **Net trade:** OAuth gives a same-UID attacker an extra HTTP round-trip; bearer gave them nothing. For personal-use, this delta is academic.
- External attacker: `127.0.0.1` bind defeats both.

If the different-UID-local-process threat ever matters, the fix is `SO_PEERCRED` on the TCP socket (`getsockopt(IPPROTO_TCP, TCP_INFO)` doesn't expose creds; we'd need `tokio-uring`-flavoured customisation or a small unix-domain side-channel for the OAuth-token endpoint). Out of scope for this ADR.

## Consequences

**Positive:**

- `~/.claude.json` is touched once: at install. Never again. No reboot gotcha. No "did install.sh run today?" troubleshooting path.
- Token rotation is automatic and transparent — daemon can re-mint internally without any client coordination.
- We become MCP-spec-2025-11-25 conformant on the authorization axis (we already are on protocol).
- The bearer-file delete + middleware simplification net out to roughly the same LOC as today (~250 in, ~150 out).
- The runbook entries that involved "rotate the token" / "token mismatch" become moot. Live diagnostics shrink to "is the daemon up? does `/oauth/token` issue? does `/mcp` accept the issued token?" — three `curl` calls.
- Token storage burden moves to Claude Code, where it should live (the client has secure-storage abstractions; the daemon's `~/.local/state` is a worse home).

**Negative / what we give up:**

- A same-UID-different-UID-local attacker now needs one fewer step. Acknowledged in the threat-model note above. We accept it because personal-use single-user.
- `packaging/install.sh` becomes simpler, but anyone running the old install.sh against this version will leave a stale `Authorization` header in `~/.claude.json`. The new install.sh should `claude mcp remove kde-mcp` before `claude mcp add` to flush. Documented in the ADR's implementation notes.
- Existing live deployments need `install.sh` re-run **once** after the upgrade to remove the now-spurious bearer header.

**Neutral / housekeeping:**

- Memory `reference-claude-code-mcp-caveats` updates: §1 (HTTPS_PROXY) stays; remove the bearer-file paragraph.
- Memory `project-daemon-deployment-state` updates: drop the "Token is rotatable" recipe, drop the `$XDG_RUNTIME_DIR/kde-mcp/token` mention.
- HANDOFF.md updates: note that `~/.claude.json` no longer carries a token after ADR-0014.

## Alternatives considered

1. **Persist the bearer to `~/.local/state/kde-mcp/token`.** Patch the reboot gotcha without changing the protocol. Rejected: keeps a non-spec-conformant transport, and leaves the same-process-can-read-the-token property unchanged. Doesn't actually solve the misuse.

2. **Daemon writes `~/.claude.json` on startup to ensure the bearer matches.** Rejected: a desktop-control daemon should not be modifying Claude Code's user config. Layer violation; surprising side effects (lockfile contention, JSON-merge edge cases). Smells wrong even when it works.

3. **stdio→unix bridge as the install default; keep the singleton daemon HTTP.** Drops the bearer entirely (Unix socket + 0600 perms replaces it). Rejected during the discussion: the user explicitly wants to preserve multi-Claude-instance sessions via ADR-0011's lease, and was wary of any architecture that smelled like per-session processes. The bridge does *not* re-introduce that (the singleton daemon stays singleton; each bridge is a transparent forwarder), but the design ergonomics — "extra binary, extra install step" — were not preferred.

4. **Wait for Claude Code to support `unix://` URLs and drop HTTP.** This is the *long-term* fix and we'll do it when upstream lands. Until then, we need a working HTTP transport, and OAuth is the right authorization for HTTP.

5. **Use `SO_PEERCRED` on the TCP loopback connection inside the auth middleware.** Linux exposes process credentials over Unix domain sockets cleanly (`SO_PEERCRED`); on TCP, the equivalent is `SO_PEERCRED` over `AF_INET`/`AF_INET6` loopback, which is supported on Linux but documented poorly. Rejected for this ADR as scope creep — it can be a future hardening step bolted on top of OAuth without protocol changes.

## Implementation notes (for the follow-up commit, not this ADR)

1. Add `src/transport/oauth.rs`. Three endpoints (well-known × 2, `/oauth/token`, `/oauth/register`), one validator middleware, one TTL sweep task. ~250 LOC including doc comments.
2. Delete `src/transport/auth.rs`. Update `src/transport/mod.rs`.
3. `src/transport/daemon.rs::http_router`: swap middleware; mount `/oauth/*` and `/.well-known/*` *outside* the middleware-protected scope.
4. Update `packaging/install.sh`: drop the `case http)` token-read block (lines 158–168 of the current file), simplify the `claude mcp add --transport http` invocation to a no-headers form, and add a `claude mcp remove --scope user kde-mcp` line before the add (idempotent; flushes stale bearer headers from prior installs).
5. Delete `tests/integration/daemon_http.rs::healthz_reports_ok_without_auth` (still valid — `/healthz` stays open) and the bearer-specific tests; add `tests/integration/oauth.rs` covering the four wire behaviours above.
6. Update `docs/specs/2026-05-15-singleton-daemon-design.md` (if it documents the bearer wire shape) — find/replace the bearer description with the OAuth reference.
7. After landing: re-run `packaging/install.sh --transport=http` on the dev machine to flush the stale bearer from `~/.claude.json`, then restart the Claude Code session. Verify `mcp-logs-kde-mcp/<latest>.jsonl` shows successful auth completion and `/oauth/token` issuance in `journalctl --user -u kde-mcp-http.service`.

A clean implementation should be net ~+100 LOC (oauth.rs in, auth.rs + install.sh + a few tests out), plus the ADR.
