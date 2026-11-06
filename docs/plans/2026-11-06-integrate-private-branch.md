# Integrate pending local work onto the public history

The public-release checkout is the canonical base. Preserve its sanitized history, contributor guidance, dependency security fixes, and Rust latency analyzer. Do not merge the old private history or copy old documents wholesale.

## Scope and sequence

- [x] Port the two server-dispatch refactor commits and the session-tag/shared-test-lock commits using their patches against the public base. Validate Rust formatting, clippy, and workspace tests.
- [x] Translate the two TypeScript analyzer commits' behavior into the existing Rust analyzer: derive the report end date from the latest valid included timestamp, fall back to the supplied current date for empty/invalid input, and use cleanup-safe isolated CLI test fixtures. Do not restore TypeScript files. Demonstrate the regression before changing implementation; run analyzer tests, formatting, and clippy.
- [x] Port the Python harness scaffold and offline smoke tests. Keep live paid API calls disabled. Describe only implemented entry points and avoid account-specific claims or private configuration examples. Run offline tests.
- [x] Port the approval fix, tests, ADR, and spec updates without reversing public redactions. Verify the native-notification tests on a private D-Bus session. Run root and standalone analyzer checks before committing.
- [ ] Review the complete public diff and commit ancestry for private paths, account-specific details, restored private documents, unexpected deletions, and security-lockfile regressions. Commit only reviewed changes with public-history parents.
- [ ] Fast-forward public main to the verified integration branch and push to the existing GitHub main without force. Keep the original checkout and its uncommitted changes intact.
- [x] Rename the old checkout to kde-mcp-old and the public checkout to kde-mcp. Use explicit canonical paths afterward; no process/session/daemon restart. Retain the old checkout for later information audit; do not delete it.

## Runtime boundary

The existing daemon continues to run the already-installed approval-fix build from the old base. Renaming repositories does not change its executable or retarget a process's current working directory. Build the integrated release and install it for the next daemon start, but do not restart the daemon without new authorization. Verify the installed file separately from the running executable and report the difference.

## Verification before publication

- Root Rust workspace: 83 unit tests and 62 integration tests passed. The notification helper runs on a private bus through its parent test; no real desktop input was sent.
- Standalone Rust latency analyzer: 22 unit tests and 3 CLI integration tests passed. The report-date regression failed before the behavior change and passed afterward.
- Offline Python harness: 5 tests passed; the paid live API test was explicitly disabled and skipped. The environment reported a pytest-asyncio deprecation warning on Python 3.14.
- Both Rust workspaces passed formatting and clippy with warnings denied.
- Public-release contributor files, README, root Cargo manifests/lockfile, transport configuration, and existing privacy cleanup were preserved. No old private commit objects were imported by patch application.
- The old checkout, its full history, ignored artifacts, and uncommitted CLAUDE.md edit remain intact under kde-mcp-old. Deletion is not authorized.
