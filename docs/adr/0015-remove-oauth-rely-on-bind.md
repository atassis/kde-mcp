# ADR-0015: Drop the OAuth layer; trust is the 127.0.0.1 bind

**Status:** Accepted (2026-05-18)

Supersedes [[0014-oauth-replaces-static-bearer]]. The singleton-daemon shape (ADR-0012), the systemd socket-activation, and the lease semantics (ADR-0011) are unchanged. Only the **authorization layer** flips again: from the OAuth 2.1 auth server we just shipped, to no application-layer auth at all. `/mcp` returns 200 to unauthenticated requests.

## Context

ADR-0014 (two days ago) replaced a static bearer-in-tmpfs with a daemon-hosted OAuth 2.1 flow. The justification was twofold: (a) MCP spec 2025-11-25 says HTTP transports SHOULD use OAuth; (b) Claude Code 2.1.139 was driving the OAuth flow regardless.

The "Trust model" section of ADR-0014 already conceded — explicitly — that OAuth provides **zero security benefit** in our threat model:

> - Same-UID malicious local process: defeated by `--transport=unix` (already implemented, blocked on upstream). Bearer was no better here.
> - Different-UID local process: defeated by the 127.0.0.1 bind. Unchanged.
> - External attacker: defeated by the 127.0.0.1 bind. Unchanged.

A same-UID process can auto-complete the `/oauth/authorize` endpoint anyway (it has no consent gesture — it's auto-approve, since the only trust signal is "the request reached us on the loopback bind"). The bearer simply gates the bearer-holder, who is the same person/process the OAuth flow just issued the bearer to. It adds no security here.

Two days of running with the OAuth code revealed the maintenance cost:

1. **Commit `134e289`** patched `/.well-known/oauth-authorization-server` to add `authorization_endpoint` + PKCE alg advertisement, because Claude Code 2.1.139's Zod validator hard-required them.
2. A second WIP fix was needed because `/oauth/register` returned no `redirect_uris`, which Claude Code's Zod also rejects:
   ```
   ZodError: expected "array" at redirect_uris, received undefined
   ```
3. A third fix was needed because the `/oauth/token` endpoint advertised `client_credentials` only, but Claude Code's MCP OAuth client uses `authorization_code` + PKCE per the MCP spec.

That's three Claude-Code-version-sensitive breakpoints inside two days. Every Claude Code release ships a tighter Zod schema; every release threatens to invalidate our OAuth surface again.

## The empirical pivot

On 2026-05-18 we built a minimal Python MCP server with **no auth** and **no `/.well-known/oauth-*` endpoints**, registered it as a Claude Code MCP entry, and ran a one-shot Claude Code subprocess against it. Result, logged in Claude Code's MCP client logs:

```
"Successfully connected (transport: http) in 23ms"
```

No `Authorization` header sent. No probe to any `/.well-known/oauth-*` URL. Claude Code's flow is:

1. `POST /mcp` (no auth) →
2. If `401` → run OAuth discovery + handshake.
3. If `200` → proceed.

Compare ADR-0014's path (8 round-trips: probe → 401 → fetch metadata × 2 → DCR → authorize → token-exchange → /mcp-with-bearer) versus 1 round-trip. The OAuth path bought us nothing in security and cost us ~280 LOC of Rust plus per-release schema breakage.

## Decision

**Delete the OAuth module. `/mcp` returns 200 to unauthenticated requests. The 127.0.0.1 bind is the trust boundary.**

### Wire-level behaviour

1. **`POST /mcp` →** whatever `rmcp` returns. No bearer required. No `Authorization` header inspected.
2. **`GET /.well-known/oauth-*` →** the daemon no longer serves these. axum returns 404. This is the signal Claude Code's MCP client uses to skip the OAuth dance.
3. **`GET /healthz` →** unchanged, open by design.

### Code shape

- Delete `src/transport/oauth.rs` (~570 lines including tests).
- Delete `tests/integration/oauth.rs` (~260 lines).
- Drop `pub mod oauth;` from `src/transport/mod.rs` and `tests/integration.rs`.
- Simplify `src/transport/daemon.rs::serve_http`: no more `OAuthState` construction, no `oauth_validate_middleware` layer, no discovery routes, no `spawn_sweeper`. Collapse the `serve_with_listener_oauth_timeout` helper into `serve_with_listener_and_timeout`.
- `packaging/install.sh` already stopped touching `~/.claude.json`'s Authorization header in ADR-0014; nothing more to do there.
- No new deps; no removed deps (base64/sha2 are used widely by audit + screenshot + clipboard).

### Trust model

Unchanged from ADR-0014's own analysis. To restate:

- **Same-UID malicious local process:** inside the threat model. They can drive libei, dbus, and `org.kde.KWin.ScreenShot2` directly without touching kde-mcp. Defended (only) by `--transport=unix` once Claude Code supports `unix://`.
- **Different-UID local process:** defeated by binding to `127.0.0.1` only. The Linux network namespace + the bind address make this unreachable from another UID's perspective absent root.
- **External-machine attacker:** defeated by the `127.0.0.1` bind.

OAuth contributed nothing to any of these. Dropping it does not regress the model.

## Consequences

**Positive:**

- ~830 lines of code deleted (oauth.rs + oauth integration tests + daemon.rs wiring).
- No more Claude-Code-version-sensitive auth surface. The MCP wire shape itself (initialize, tools/list, tools/call) is stable across releases; the OAuth metadata shapes were not.
- Faster connect: 1 round-trip vs 8.
- `/.well-known/oauth-*` discovery URLs are gone — there's less attack surface for any client that probes them.
- Audit log is no longer cluttered with `daemon: OAuth issuer ready` and per-mint events that didn't carry security value.

**Negative:**

- Spec-strictness: MCP 2025-11-25 *recommends* OAuth for HTTP transports. We're not violating it (recommendation, not requirement) but we no longer announce conformance via discovery. Clients that **require** OAuth (none we've seen) would not connect.
- Future Claude Code releases could in principle reverse course and require OAuth. If that happens we re-instate the OAuth layer; the prior code is preserved in the git history at commit `134e289` and the parent of this ADR's commit.
- Anyone reading ADR-0014 today must follow the `Supersedes by` pointer here.

## Alternatives considered

1. **Keep ADR-0014 as-is.** Rejected: adds no security here, and ongoing maintenance cost.
2. **Ship `--transport=unix`-only and wait for Claude Code `unix://` support.** Tempting, but the upstream change is out of our hands. Until that lands we still want HTTP-on-loopback to work for the user's actual daily workflow. Drop the auth on the HTTP transport; keep the Unix transport ready to flip on.
3. **Implement a minimal bearer that's never advertised via `/.well-known`.** Worse than this option in every dimension: still maintains shared-secret sync logic, gives the same zero security on the threat model, and trips Claude Code's "needs-auth" detection.

## Verification

Empirical test artefacts:
- A minimal no-auth Python MCP server used as the prober.
- Claude Code's MCP client log showing the 23 ms unauthenticated connect.
- `tests/integration/daemon_http.rs::http_initialize_returns_serverinfo` already covered the unauth happy path; no new tests needed for the deletion.

## How to apply

After deploy:
1. `cargo build --release`
2. `cp target/release/kde-mcp ~/.local/bin/kde-mcp`
3. `systemctl --user restart kde-mcp-http.service`
4. `curl -sS -X POST http://127.0.0.1:7575/mcp -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'` — should return 200 with `kde-mcp` serverInfo, no `WWW-Authenticate`.
5. Restart Claude Code session to clear the cached "needs-auth" state from prior OAuth attempts.
