# ADR-0007: Adopt Anthropic computer-use safety architecture

**Status:** Accepted
**Date:** 2026-05-13
**Deciders:** atassis

## Context

After ADRs 0001–0006 were settled and the initial design was committed, we discovered that Anthropic's Claude Code CLI ships a built-in `computer-use` MCP server (research preview, macOS only) with sophisticated safety features documented at https://code.claude.com/docs/en/computer-use.

The relevant features Anthropic ships:

1. **Per-app approval per session** — first time Claude needs a specific app, a terminal prompt lists target apps, extra permissions, and how many apps will be hidden; user picks Allow / Deny.
2. **App category tiering by risk** — browsers and trading platforms → view-only; terminals and IDEs → click-only; everything else → full control.
3. **Sentinel warnings** — apps with broad reach (terminals, Finder, System Settings) flagged in the approval prompt.
4. **Hidden non-approved apps during a turn** — Claude only sees the apps it was approved for.
5. **Terminal excluded from screenshots** — agent never sees its own output, so on-screen prompts in the session can't feed back as prompt injection.
6. **Global Esc to abort, key consumed** — Esc from anywhere aborts; the key is consumed so prompt injection can't dismiss dialogs.
7. **Single-session machine-wide lock** — only one session can control the machine at a time; notifications on acquire / release.
8. **Bundled with the host** — no separate install, enable via `/mcp`.
9. **Server-side prompt-injection classifier** — Anthropic-side checking of on-screen content for injection.

The original kde-mcp design covered structural defenses (annotations, audit log, blocklist, sensitive-path warnings, trust wrappers) but did not match Anthropic's UX-level safety polish.

## Decision

Adopt six of Anthropic's features as core MVP scope, implementing them as a cross-cutting **policy gate** (see spec §6.0). Skip three for capacity / scope reasons; substitute structural defenses where possible.

### Adopted (6)

| Feature | Implementation | Spec ref | Est. effort |
|---|---|---|---|
| Single-session lock + notifications | POSIX flock at `$XDG_RUNTIME_DIR/kde-mcp/<mode>.lock` per mode (live + virtual are separate locks). `org.freedesktop.Notifications` on acquire/release. PID liveness for stale detection. | §10 layer 3 | 0.5 day |
| Per-app approval per session | In-memory `HashMap<app_class, ApprovalState>`. First destructive tool against an unapproved app returns `app_approval_required` error and fires a notification with `Allow once` / `Allow this session` / `Deny` action buttons. State updates from user click; agent retries. | §10 layer 4, §6.0 step 5 | 3 days |
| App-category tiering | TOML table at `~/.config/kde-mcp/policy.toml`. Three tiers: `view_only`, `click_only`, `full`. Per-tool enforcement (typing blocked in `click_only`, all destructive blocked in `view_only`). User-overridable. | §10 layer 5, §6.0 step 4 | 1.5 days |
| Sentinel warnings | Same TOML table tags apps with sentinels (`shell_access`, `filesystem_access`, `system_settings`). Destructive tool responses gain `warnings: [...]` when targeting them. | §10 layer 6, §6.0 step 6 | 0.5 day |
| Terminal-excluded screenshots | Detect terminal-class windows via KWin Scripting; mask their bbox with a solid black rectangle + label before returning PNG. | §10 layer 7 | 1 day |
| Global cancel shortcut | Register `Meta+Esc` (configurable) via KDE KGlobalAccel D-Bus. KWin grabs the key at compositor level (apps don't see it). Sets a `CancellationToken` polled by every destructive tool at safe points. | §10 layer 11, §6.0 step 2 | 2–3 days |

Total: ~8–9 working days of additional MVP scope.

### Skipped (3)

| Feature | Why skipped | Substitute |
|---|---|---|
| Apps hidden while Claude works | Requires host-level turn awareness we don't have as an external MCP server. Implementing it via per-tool window minimization is visually disruptive; via a `session.begin/end_turn` tool the agent would forget to call. | Per-app approval (layer 4) + tier restriction (layer 5). The agent can still see other apps but can't act on them without approval. Different security property but acceptable for personal use. |
| Server-side prompt-injection classifier | We have no classifier weights, no training data, no inference budget. Anthropic runs this in their infrastructure. | Trust wrappers (layer 10) + sensitive-path warnings (layer 9) + per-app approval (layer 4) which adds a human-in-the-loop checkpoint. |
| Bundled-with-host install | We're not Anthropic; we don't ship Claude Code. | Ship a `.mcpb` extension bundle + a `claude plugin install` registration script. One-command install is the closest substitute. (Out of MVP, in polish phase.) |

## Rationale

- **The features are reimplementable on KDE Wayland.** KDE provides `org.freedesktop.Notifications` for actionable prompts, KGlobalAccel for compositor-level shortcut grabs, KWin Scripting for window enumeration and class queries. None of the adopted features require Anthropic-specific infrastructure.
- **Personal-use stance still holds.** ADR-0002 said personal MVP first. The user (atassis) explicitly asked for the safety upgrades after seeing the Anthropic feature list and accepted the ~8–9 added days.
- **Policy gate is a single cross-cutting subsystem.** Implementing the six features as ad-hoc per-tool logic would distribute the safety story across 7 destructive tools. Centralizing in `src/policy.rs` keeps the security-critical code in one auditable place.
- **The substitutes are honest.** We don't claim parity with Anthropic on the three skipped features; the spec §10 spells out the gaps. Future work may close some of them.

## Consequences

- **MVP grows from 12–16 working days to 18–23 working days** (5–7 calendar weeks at part-time pace) — a deliberate trade of scope for quality.
- **New cross-cutting subsystem**: `src/policy.rs` plus thin helpers `src/lock.rs`, `src/notify.rs`, `src/cancel.rs`. Each destructive tool wraps its execution in a single `policy.gate(...)` call. Adding new destructive tools is a one-line addition to the gate-wrapped pattern.
- **New error codes**: `tier_restricted`, `app_approval_required`, `cancelled`, `lock_unavailable` (see spec §8).
- **New config surface**: `~/.config/kde-mcp/policy.toml`. Ships with sensible defaults; user-overridable.
- **New runtime dependency on `kglobalaccel`** (always present on KDE Plasma 6) and `org.freedesktop.Notifications` (always present).
- **The "are we better than Anthropic" framing shifts.** With these adopted, our remaining structural disadvantages are the three skipped features. The comparison doc (`docs/comparison-with-anthropic-computer-use.md`) is updated accordingly.

## Alternatives considered

- **Skip all safety features beyond the original five layers.** Rejected — the upgrade was a deliberate choice, accepting the extra time.
- **Adopt a subset (e.g., just lock + tiering, skip per-app approval).** Considered but rejected: per-app approval is the most user-visible safety property in Anthropic's design; skipping it would leave the most obvious gap.
- **Implement features ad-hoc per tool rather than centralised gate.** Rejected: security-critical code should not be scattered. The gate gives us one place to audit, test, and extend.

## Open questions

- **Cross-process approval persistence.** Current design: in-memory, lost on restart. Future enhancement: optional `~/.local/state/kde-mcp/approvals.json` with TTL.
- **Notification fallback for non-KDE-notification setups.** Some users may run with `dunst` or similar. `org.freedesktop.Notifications` is a freedesktop spec, not KDE-specific, so should work — but the action-button protocol coverage varies. Test before claiming support.
- **`.mcpb` packaging.** Anthropic recently introduced Claude Desktop Extension format. Investigate before MVP polish phase.

## Source

This ADR follows a design review of the Claude Code computer-use documentation (https://code.claude.com/docs/en/computer-use) on 2026-05-13. The driving question: could the safety features be reimplemented on KDE primitives at acceptable cost? The decision was to spend more MVP time in exchange for a result close to — or better than — Anthropic's in quality. This ADR records the triage and the commitment to build the six adopted features in MVP.
