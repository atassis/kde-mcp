# Phase 0 Smoke Test Procedure

Manual verification of the `kde-mcp` Phase 0 build against a real KDE Plasma 6 / Wayland session. Run this after every Phase 0 commit, and as the entry sanity check for Phase 1+ development.

The procedure has two parts: a **scripted JSON-RPC handshake** (fast, no host required) and an optional **`claude mcp add` register-and-discover** flow (validates the host-integration path).

---

## Preconditions

- KDE Plasma 6 / Wayland session:

  ```sh
  test "$XDG_SESSION_TYPE" = wayland && echo OK
  test "$KDE_SESSION_VERSION" = 6 && echo OK
  ```

- No other `kde-mcp` already running with the same mode (lockfile would block):

  ```sh
  pgrep -af kde-mcp || true
  ```

- Release binary built:

  ```sh
  cargo build --release
  test -x target/release/kde-mcp && echo OK
  ```

---

## Part A — Scripted JSON-RPC handshake (no Claude Code needed)

The MCP wire protocol is newline-delimited JSON-RPC over stdio. We pipe a fixed sequence to the binary and check the responses.

### A.1 Run the fixture

Save as `/tmp/kde-mcp-smoke.sh`:

```sh
#!/usr/bin/env bash
set -euo pipefail
BIN="$1"
{
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0"}}}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"observe","arguments":{}}}'
  sleep 0.5
} | timeout 8 "$BIN" --mode=live 2>/tmp/kde-mcp-smoke.stderr
```

Then:

```sh
chmod +x /tmp/kde-mcp-smoke.sh
/tmp/kde-mcp-smoke.sh "$PWD/target/release/kde-mcp" > /tmp/kde-mcp-smoke.stdout
echo "exit=$?"
```

### A.2 Pass criteria

1. **Exit code 0.**
2. **Stdout contains exactly 3 newline-delimited JSON objects** (one per request — the `notifications/initialized` notification has no response).

   ```sh
   wc -l /tmp/kde-mcp-smoke.stdout  # → 3
   ```

3. **Response 1 — `initialize`:**
   - `result.protocolVersion == "2025-11-25"`
   - `result.serverInfo.name == "kde-mcp"`, `result.serverInfo.version == "0.1.0"`
   - `result.capabilities.tools` is present (may be empty object `{}`)

   ```sh
   sed -n '1p' /tmp/kde-mcp-smoke.stdout | python3 -c "import sys,json; d=json.load(sys.stdin)['result']; \
     assert d['protocolVersion']=='2025-11-25', d; \
     assert d['serverInfo']['name']=='kde-mcp', d; print('init OK')"
   ```

4. **Response 2 — `tools/list`:**
   - Exactly **16 tools** are returned, with these wire names (order may vary, set should match):

     ```
     act_on_element, bash, clipboard_read, clipboard_write,
     inspect, keyboard.key, keyboard.type, mouse,
     observe, read_text, screenshot,
     text_editor.edit, text_editor.view, wait,
     window_control, window_query
     ```

   - Each tool has an `inputSchema` JSON Schema object (with `properties`, `required` array where applicable).
   - **Observation tools** (`observe`, `inspect`, `screenshot`, `read_text`, `wait`, `window_query`, `clipboard_read`, `text_editor.view`) carry `annotations.readOnlyHint == true`.
   - **Mutating tools** (`act_on_element`, `mouse`, `keyboard.type`, `keyboard.key`, `window_control`, `clipboard_write`, `text_editor.edit`) carry `annotations.destructiveHint == true`.
   - **`bash`** carries both `annotations.destructiveHint == true` and `annotations.openWorldHint == true`.

   ```sh
   sed -n '2p' /tmp/kde-mcp-smoke.stdout | python3 - <<'PY'
   import sys, json
   d = json.load(sys.stdin)['result']
   tools = {t['name']: t for t in d['tools']}
   expected = {
     "observe","inspect","screenshot","read_text","wait",
     "act_on_element","mouse","keyboard.type","keyboard.key",
     "window_query","window_control","clipboard_read","clipboard_write",
     "bash","text_editor.view","text_editor.edit",
   }
   assert set(tools) == expected, f"diff: {set(tools) ^ expected}"
   ro = {"observe","inspect","screenshot","read_text","wait","window_query","clipboard_read","text_editor.view"}
   for n in ro:
       assert tools[n]['annotations'].get('readOnlyHint') is True, n
   destr = {"act_on_element","mouse","keyboard.type","keyboard.key","window_control","clipboard_write","bash","text_editor.edit"}
   for n in destr:
       assert tools[n]['annotations'].get('destructiveHint') is True, n
   assert tools['bash']['annotations'].get('openWorldHint') is True
   print("tools/list OK")
   PY
   ```

5. **Response 3 — `tools/call observe`:**
   - `result.isError == true`
   - `result.content[0].text` contains `"not implemented"`
   - `result.content[1].text` is JSON parseable; `code == "not_implemented"`

   ```sh
   sed -n '3p' /tmp/kde-mcp-smoke.stdout | python3 -c "import sys,json; d=json.load(sys.stdin)['result']; \
     assert d['isError'] is True; \
     payload=json.loads(d['content'][1]['text']); \
     assert payload['code']=='not_implemented', payload; print('observe error OK')"
   ```

### A.3 Side-effect checks

- **Lockfile** was created and (after process exit) still exists with the binary's PID as its contents:

  ```sh
  test -f "$XDG_RUNTIME_DIR/kde-mcp/live.lock" && \
    test -s "$XDG_RUNTIME_DIR/kde-mcp/live.lock" && echo "lockfile OK"
  ```

  *(The file persists between runs by design — `fd-lock` releases the advisory lock on close, leaving the file as a stale PID record.)*

- **Audit log file** was created (empty in Phase 0; tool-dispatch wiring lands in Phase 1):

  ```sh
  test -f ~/.local/state/kde-mcp/audit.jsonl && echo "audit log path OK"
  ```

- **KDE notifications** "Claude is using your computer" and "Claude finished using your computer" should appear on screen during the run. This is visual; automate only if KDE's notification daemon supports query-the-history (out of scope).

- **Concurrent-launch contention** — start two `--mode=live` instances; the second should fail with `lock_unavailable`:

  ```sh
  target/release/kde-mcp --mode=live &
  sleep 0.3
  target/release/kde-mcp --mode=live   # expect: Error: lock unavailable; held by pid N
  kill %1
  wait %1 2>/dev/null || true
  ```

### A.4 What pass means

If A.1–A.3 succeed, the binary:

- speaks MCP 2025-11-25 correctly,
- advertises the right tool surface with the right schemas and hints,
- maps `ToolError::NotImplemented` to a structured `{code, details}` payload the model can parse,
- holds the per-mode lockfile, writes the holder PID, releases on exit,
- opens the audit log path and creates parent directories,
- fires the acquire / release notifications.

Phase 0 is functionally green.

---

## Part B — `claude mcp add` registration (Claude Code interactive)

Use this to validate the host-integration path. Requires Claude Code installed.

### B.1 Register

```sh
claude mcp add kde-mcp-phase0 "$PWD/target/release/kde-mcp" -- --mode=live
claude mcp list
```

Expected: `kde-mcp-phase0` appears.

### B.2 Discover

Inside a Claude Code conversation: ask "list the tools available from `kde-mcp-phase0`". Expected: all 16 wire names appear (the model may collapse pairs in prose; the underlying list is 16).

### B.3 Invoke

Inside Claude Code: ask "call `kde-mcp-phase0`'s `observe` tool with no arguments". Expected: the model surfaces "this tool is a Phase 0 stub" or the `not_implemented` code.

### B.4 Unregister

```sh
claude mcp remove kde-mcp-phase0
```

Expected: the "Claude finished using your computer" notification fires when the host disconnects from the server.

---

## Known not-yet-implemented

These are Phase 1+ work and **not** failures during Phase 0 smoke:

- Audit log is empty after a tool call (tool-dispatch audit hookup lands in Phase 1's first task).
- `--mode=virtual` exits with `not_implemented` from `mode::r#virtual::bootstrap` (Phase 5 implements it).
- Every `tools/call` returns `not_implemented`. The infrastructure is what Phase 0 ships; the tool bodies fill in Phases 1–4.

If any of the above produces *unexpected* behaviour (e.g. `--mode=virtual` panics instead of cleanly returning `not_implemented`), that **is** a Phase 0 regression.
