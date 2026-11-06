# Wait for app approval within the original tool call

Status: Approved for implementation. User selected a 60-second approval deadline and the shared-gate approach, including lease and target-safety changes.

## Problem and evidence

`PolicyGate::check` in `src/policy.rs` sends a notification and immediately returns `AppApprovalRequired`. Both absent and Pending entries send a fresh notification. `src/notify.rs` updates the approval map asynchronously, so only a subsequent tool call sees the decision. This implements the original spec §6.0 and ADR-0007, not the desired wait-and-resume interaction.

## Decision

Keep the original tool call pending for up to 60 seconds. An approval resumes that call; no agent retry is required. Maintain one active approval notification per app, including when callers overlap. Session approval retains existing persistence semantics; changing persistence is out of scope. Once approval permits exactly one call, never all concurrent waiters.

Deny or dismissal returns permission_denied. Expiry returns timeout. Meta+Esc returns cancelled promptly. Notification delivery failure fails closed with an actionable error rather than waiting for an impossible response. No side effects occur on these paths. Client-side deadlines may still end a call earlier.

## Lifecycle and trust boundaries

The gate owns pending-request identity and approval-state transitions. Notification code reports decisions to that request rather than independently granting approvals through an untracked detached listener. A late response to an expired or cancelled request cannot authorize a later call. Close obsolete notifications and stop their listeners. A dropped request must not leave an unusable Pending entry. Concurrent waiters must not duplicate notifications or consume the same Once grant.

Do not hold the approval-map mutex while waiting for the user. Waits are asynchronous and cancellation-aware. The caller deadline includes delivery. Drain in-flight delivery after cancellation so its eventual notification id can be closed; cleanup is best-effort if the service is unavailable. Recheck cancellation before returning permission to execute.

## Necessary dependent safeguards

The current lease idle timeout is 10 seconds. Preserve mutation ownership across the approval wait and fail closed if ownership is lost; do not resume using an expired lease. Keep any lease maintenance bounded to the lifetime of the active call.

Keyboard input targets the focused window; mouse targeting depends on current window placement/focus. Revalidate the resolved target after the approval wait and before input. If the target changed, fail without input rather than silently transferring approval or refocusing a different window. Ref-addressed actions must still address their original object and fail if it no longer exists. Do not broaden this into a redesign of existing target-resolution fallbacks.

## Verification

Automated tests must use a controllable notification backend, not the user's live notification service. Cover: call remains pending before approval; the same call proceeds after approval; denial/dismissal; deadline expiry; cancellation; notification delivery failure; concurrent requests send one notification; Once permits one action; cleanup and late-response rejection; lease ownership across a wait longer than its idle timeout; target changes prevent input.

Run workspace tests, formatting, and clippy. Live verification requires a batched notification round trip and the original window-focus scenario against the rebuilt binary. Do not claim deployed behavior from unit tests. Installing a binary or restarting the shared daemon requires separate authorization.

## Documentation and scope

Record the changed approval interaction in a new ADR without rewriting accepted ADR history, and update the active spec's approval-flow sections. Preserve unrelated working-tree edits (currently CLAUDE.md). No commit, push, deployment, or daemon restart is authorized by this design discussion.
