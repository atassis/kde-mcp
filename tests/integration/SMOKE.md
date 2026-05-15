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

- **Concurrent-launch coexistence** — start two `--mode=live` instances; both should start successfully. Mutating tool calls from session B should block (returning `busy` after `acquire_timeout` if A holds for longer than the timeout):

  ```sh
  target/release/kde-mcp --mode=live &
  sleep 0.3
  target/release/kde-mcp --mode=live &   # expect: starts; both processes running
  sleep 0.3
  ls -la "$XDG_RUNTIME_DIR/kde-mcp/lease.json" 2>/dev/null || echo "(lease appears only when a mutating tool is in flight)"
  kill %1 %2 2>/dev/null
  wait %1 %2 2>/dev/null || true
  ```

### A.4 What pass means

If A.1–A.3 succeed, the binary:

- speaks MCP 2025-11-25 correctly,
- advertises the right tool surface with the right schemas and hints,
- maps `ToolError::NotImplemented` to a structured `{code, details}` payload the model can parse,
- starts cleanly with no startup lock (per ADR-0011); mutation lease is per-call, opened on demand,
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

These are Phase 2+ work and **not** failures during Phase 1 smoke:

- `--mode=virtual` exits with `not_implemented` from `mode::r#virtual::bootstrap` (Phase 5 implements it).
- Mutating tools (`mouse`, `keyboard.{type,key}`, `act_on_element`, `window_control`, `clipboard_write`, `text_editor.edit`, `bash`) and `read_text` / `wait` return `not_implemented`. Tool bodies land in Phases 2–4.
- `inspect target="focused"` returns `not_implemented` — focus tracking via AT-SPI `FocusEvents` lands in Phase 4 polish.
- `inspect target="window:<ref>"` returns `not_implemented` — window→AT-SPI app mapping is Phase 4.
- `observe.focused_element` is always `null` with a warning — same reason as above.

If any of the above produces *unexpected* behaviour (panics, malformed errors, missing audit records), that **is** a Phase 1 regression.

---

## Part C — Phase 1 read-only tools

These require a live KDE Plasma 6 / Wayland session. Each `--mode=live` invocation fires the "Claude is using your computer" KDE notification.

### C.1 — `text_editor.view`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"text_editor.view","arguments":{"path":"/etc/os-release","view_range":[1,3]}}}'; \
  sleep 0.5; } | timeout 5 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c1.jsonl
test "$(wc -l < /tmp/c1.jsonl)" = 2 && echo "C.1 line count OK"
sed -n '2p' /tmp/c1.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["kind"] == "file", payload
assert "1\t" in payload["content"], payload["content"][:200]
print("C.1 OK")'
```

### C.2 — `clipboard_read`

Requires `wl-clipboard` installed. Copy "kde-mcp test" into the clipboard via any normal means first.

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"clipboard_read","arguments":{"mime_type":"text/plain"}}}'; \
  sleep 0.5; } | timeout 5 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c2.jsonl
sed -n '2p' /tmp/c2.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["content"]["trust"] == "external", payload
assert payload["origin"] == "external", payload
print("C.2 OK; clipboard content was:", repr(payload["content"]["text"][:80]))'
```

### C.3 — `window_query`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"window_query","arguments":{"action":"list","format":"titles"}}}'; \
  sleep 1.0; } | timeout 6 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c3.jsonl
sed -n '2p' /tmp/c3.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert "windows" in payload, payload
print(f"C.3 OK; {len(payload[\"windows\"])} windows visible")
for w in payload["windows"][:5]:
    print(" -", w["uuid"], w["class"]["text"], "|", w["title"]["text"][:40])'
```

### C.4 — `screenshot region=active_screen`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"screenshot","arguments":{"region":"active_screen","max_long_edge":640}}}'; \
  sleep 1.0; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c4.jsonl
sed -n '2p' /tmp/c4.jsonl | python3 -c '
import sys, json, base64
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["format"] == "png"
b = base64.b64decode(payload["png_base64"])
assert b[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
print(f"C.4 OK; {payload[\"returned_size\"]} from {payload[\"original_size\"]} ({len(b)} bytes)")'
```

### C.5 — `observe detail=interactive`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"observe","arguments":{"detail":"interactive"}}}'; \
  sleep 1.0; } | timeout 6 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/c5.jsonl
sed -n '2p' /tmp/c5.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["mode"] == "live"
assert isinstance(payload["cursor"], list)
print(f"C.5 OK; cursor={payload[\"cursor\"]}, focused={payload.get(\"focused_window\", {}).get(\"class\", {}).get(\"text\") if payload.get(\"focused_window\") else None}, {len(payload.get(\"windows\", []))} other windows")
for w in payload["warnings"]:
    print("  warn:", w)'
```

### C.6 — `inspect target="element:<ref>"`

Two-step: get a window's UUID from `observe`, then call `inspect`:

```sh
# Step 1: observe → grab a window UUID. Use kate or konsole or kcalc as test target.
UUID=$({ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"observe","arguments":{"detail":"interactive"}}}'; \
  sleep 1; } | timeout 6 ./target/release/kde-mcp --mode=live 2>/dev/null | sed -n '2p' | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
fw = payload.get("focused_window")
print(fw["uuid"] if fw else "", end="")')
echo "got window uuid: $UUID"
# Note: window:<ref> in inspect is Phase 4 — this is a placeholder; the
# element: form requires a previous inspect to mint the ref. End-to-end
# verification of element: is a Phase 4 SMOKE addendum.
```

`inspect target="focused"` and `inspect target="window:<ref>"` currently return `not_implemented` per the Phase 1 first-cut scope (recorded in HANDOFF.md and the plan's Task 12 step).

### C.7 — Audit log per call

```sh
truncate -s 0 ~/.local/state/kde-mcp/audit.jsonl
# Run any of C.1–C.5 above, then:
test -s ~/.local/state/kde-mcp/audit.jsonl && echo "audit non-empty"
python3 -c '
import json
records = [json.loads(line) for line in open(__import__("os").path.expanduser("~/.local/state/kde-mcp/audit.jsonl"))]
print(f"audit: {len(records)} record(s)")
for r in records:
    print(" -", r["tool"], "ok" if r["ok"] else f"err={r[\"error\"][\"code\"]}", f"{r[\"duration_ms\"]}ms")'
```

Expected: one record per `tools/call`, including the wire tool name (`text_editor.view` not `text_editor_view`).

---

## Part D — Phase 2 mutating tools

Each `--mode=live` call fires a KDE notification. Spec §14 row 2 demo target is "open kcalc, type 2+3, press =, read the result"; the fixtures below exercise pieces.

### D.1 — `keyboard.key Ctrl+Tab` in a focused browser

Focus a browser tab with multiple tabs open, then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.key","arguments":{"keys":"ctrl+Tab"}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d1.jsonl
sed -n '2p' /tmp/d1.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["ok"]
print("D.1 OK")'
```

Expected: the browser switches tabs.

### D.2 — `clipboard_write` + verify via `clipboard_read`

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"clipboard_write","arguments":{"content":"phase-2-write-test"}}}' \
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"clipboard_read","arguments":{}}}'; \
  sleep 0.8; } | timeout 6 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d2.jsonl
test "$(wc -l < /tmp/d2.jsonl)" = 3
sed -n '3p' /tmp/d2.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["content"]["text"] == "phase-2-write-test", payload
print("D.2 OK")'
```

### D.3 — `mouse click` on a Brave tab

Focus Brave first. Then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mouse","arguments":{"action":"click","coordinate":[400,20]}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d3.jsonl
sed -n '2p' /tmp/d3.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
assert d.get("isError") is not True, d
payload = json.loads(d["content"][0]["text"])
assert payload["ok"]
print("D.3 OK")'
```

### D.4 — `keyboard.type` short ASCII string

Focus a text field (URL bar, terminal, kate), then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.type","arguments":{"text":"hello"}}}'; \
  sleep 1.5; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d4.jsonl
sed -n '2p' /tmp/d4.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["used_clipboard"] == False, payload
print("D.4 OK")'
```

### D.5 — `keyboard.type` triggers clipboard-paste for non-ASCII

Focus a text field, then:

```sh
{ printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"keyboard.type","arguments":{"text":"Привет"}}}'; \
  sleep 2.0; } | timeout 8 ./target/release/kde-mcp --mode=live 2>/dev/null > /tmp/d5.jsonl
sed -n '2p' /tmp/d5.jsonl | python3 -c '
import sys, json
d = json.load(sys.stdin)["result"]
payload = json.loads(d["content"][0]["text"])
assert payload["used_clipboard"] is True, payload
print("D.5 OK")'
```

### D.6 — `act_on_element` (deferred to Phase 4)

Requires a previous `inspect` to mint an element ref. Current Phase 1 `inspect target=focused` and `inspect target=window:<ref>` return `not_implemented`. End-to-end verification of `act_on_element` therefore waits for Phase 4's focus tracker; the code path here is exercised by compile-only verification + spec parity tests.
