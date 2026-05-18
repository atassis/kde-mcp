# kde-mcp latency analyzer

Reads `~/.local/state/kde-mcp/audit.jsonl` and emits a markdown report
with p50/p95/p99 per tool, broken into the five phases captured by
the daemon (see `src/audit.rs::AuditTimings`).

## Install

    cd tools/latency-analyzer
    npm install

## Usage

    npm run --silent report -- --since 7d > docs/latency-profile.md

The `--silent` flag suppresses npm's two-line preamble (`> kde-mcp-... report`)
so the saved file contains only the markdown report.

Flags:
- `--since DURATION` — only consider records younger than DURATION
  (`1h`, `7d`, `30d`). Default: `all`.
- `--audit PATH` — override the default audit-log path.

## Tests

    npm test
