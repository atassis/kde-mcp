# App Approval Wait Implementation Plan

**Goal:** Resume the original tool call after native notification approval, bounded to 60 seconds.

**Architecture:** The policy gate owns shared pending requests with async decision delivery and cancellation-on-last-waiter-drop. Notification code supplies decisions, not independent map mutations. Dispatch maintains mutation leases while futures run; input callers validate stable target identity after the gate.

**Tech stack:** Rust, Tokio watch/oneshot channels and cancellation tokens, notify-rust, existing KWin scripts.

## Tasks

- [x] Add a failing regression in `src/policy.rs`: start `check`, assert it has not returned before user approval, record Session, assert that same future succeeds. Run `cargo test --lib policy::tests` and confirm the old immediate-error failure. Keep tests off the user's notification bus using an injected notifier.
- [x] Replace fire-and-forget map mutation in `src/notify.rs` with a decision-returning async notification lifecycle. Close notifications on cancellation, dismissal, or completion. Surface delivery failures. Confirm notify-rust's actual close/wait API before implementation.
- [x] Implement shared pending-request coordination in `src/policy.rs`, a 60-second deadline, Session/Once/Deny handling, late-response rejection and last-waiter cleanup. Preserve existing Session persistence. Exercise concurrent waiters, timeout, cancellation, dismissal and delivery failure using controlled notification responses.
- [x] Add a lease regression using the existing temporary runtime-directory convention: run a mutation longer than the idle lease and assert ownership stays fresh. Maintain and validate ownership in `src/server.rs` while its mutation future runs; stop maintenance on future drop/completion. Run server and lease tests.
- [x] Add pure target-identity comparison tests in `src/policy_target.rs`; include KWin window UUID with class in dynamic snapshots. In `src/tools/keyboard.rs` and `src/tools/mouse.rs`, compare snapshots before/after approval and reject changes before input. Retain explicit ref-addressing in window/accessibility actions and fail if the original object disappears.
- [x] Add ADR-0016, update `docs/adr/README.md` and active spec §6.0/§10 approval contract. Do not rewrite ADR-0007.
- [x] Run `cargo fmt --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`; review all changes against the approved design. Report any unavailable live smoke test. Do not deploy, restart, stage, or commit.

## Verification boundaries

The real Kate focus-and-approve scenario remains a live check against a rebuilt binary; offline tests do not establish deployed behavior. Preserve the pre-existing CLAUDE.md edit. Execute directly; no delegated work is authorized.

## Results

- Original-call regression failed on the old immediate-error implementation and passes with wait-and-resume.
- `dbus-run-session -- cargo test --workspace`: 83 library tests and 62 integration tests passed. The isolated notification helper is marked ignored for direct discovery and is run successfully by its parent test on a private bus.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --check` and `git diff --check`: passed.
- `cargo build`: passed. Isolated stdio smoke of the rebuilt debug binary passed initialize, 12-tool discovery, structured missing-KWin error, and clean shutdown. Private config/state/runtime directories were used; no shared daemon restart or install.
- The real Plasma notification and Kate window-focus scenario is not yet verified. The private-bus tests exercise native notification API actions/dismissal/cancellation/closing and a gate-to-side-effect marker, not KWin focus or EIS input.
- Existing CLAUDE.md changes were left untouched. No commit, staging, deployment, or restart performed.
