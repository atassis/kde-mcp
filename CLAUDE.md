@AGENTS.md

## Claude Code specifics

- The MCP daemon binds `127.0.0.1:7575`. Claude Code 2.1.x routes loopback through `HTTPS_PROXY`, so set `NO_PROXY=127.0.0.1,localhost,::1` in the Claude Code shell env or the client won't connect. Details in `docs/MCP-SETUP.md`.
- Pre-approve only the read-only tools in `~/.claude/settings.json`; let the destructive ones go through the policy gate's per-app approval.
