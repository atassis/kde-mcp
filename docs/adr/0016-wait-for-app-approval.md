# ADR-0016: Wait for native app approval in the original tool call

**Status:** Accepted
**Date:** 2026-11-06
**Deciders:** atassis

Supersedes the error-and-retry approval interaction in ADR-0007, not its tiering, sentinel, or other safety decisions.

## Context

The first mutation against an unapproved app returned `app_approval_required` immediately. The native notification updated shared approval state only for a later retry. Pending retries sent additional notifications, so agents could repeatedly fail before the user had time to answer.

The user requested wait-and-resume behavior and chose a 60-second deadline over an indefinite wait.

## Decision

Keep the original call pending asynchronously for up to 60 seconds, including notification delivery. Approval resumes that call without a retry. Overlapping callers share an app's pending request; Allow once admits exactly one caller. Others need another approval within their original deadline. Session approval retains existing persistence semantics.

Deny or dismissal returns `permission_denied`; an unanswered request returns `timeout`; global abort returns `cancelled`. Delivery failures fail closed. Native notification code reports decisions to a request, not directly to the grant store. Dropping the last waiter cancels the prompt and its listener; a late decision cannot grant permission to a successor request. Closing a notification is best-effort when the notification service is unavailable. An in-flight delivery is drained so its eventual notification id can be closed even if callers have already left.

Dispatch refreshes mutation ownership while a call is in flight and refuses to revive expired ownership. Input handlers compare window identity and geometry before/after the gate; coordinate-less pointer actions also compare the pointer position, and modifier actions compare focus. Changed targets fail without input rather than implicitly refocusing or transferring approval. Window-ref actions continue to address the original window and report disappearance instead of success.

## Rationale

The notification is a human consent checkpoint, not an agent-retry protocol. Waiting where consent is checked makes the normal successful response describe the requested action's actual completion. Maintaining the lease and validating dynamic targets are necessary because a human response can take much longer than an ordinary gate check.

## Consequences

- The caller needs enough time for the server's 60-second approval wait, plus other work such as lease acquisition. Client timeouts can still end a call sooner.
- Cancellation, dropped futures, concurrent waiters, and notification cleanup are explicit lifecycle responsibilities.
- Allow once can be consumed by a call that later fails target revalidation; safety takes precedence over preserving a permit.
- Existing Session approval persistence is unchanged; revisiting its scope or lifetime is separate work.
- Automated tests use a controlled notifier and a mock notification service on a private D-Bus session. They do not prove behavior of the deployed Plasma notification service.

## Alternatives considered

- **Keep immediate errors and ask agents to retry:** rejected; it leaves timing to agents and encourages duplicate calls/notifications.
- **Wait indefinitely:** rejected by the user in favor of 60 seconds; clients may have shorter lifetimes anyway.
- **Change only window_control:** rejected by the user in favor of consistent shared-gate behavior.
