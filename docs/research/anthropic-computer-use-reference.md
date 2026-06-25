# Anthropic Computer Use — Reference Notes

Collated facts about Anthropic's Computer Use tool, captured here so the design doc can stay focused.

> **Attribution & licensing.** Code snippets in this note are derived from Anthropic's MIT-licensed `anthropic-quickstarts` repository (https://github.com/anthropics/anthropic-quickstarts), specifically its `computer-use-demo` reference implementation. Prose passages are paraphrased summaries of Anthropic's public documentation (https://platform.claude.com/docs), reproduced here for reference; the linked originals are authoritative.

---

## Tool inventory

Computer Use ships as a **bundle of three Anthropic-schema client tools**: `computer`, `bash`, `text_editor` (renamed `str_replace_based_edit_tool` in newer versions). The schemas are **built into the model and not editable** — the dev sends `{"type": "computer_20251124", ...}` with **no `input_schema`**.

### `computer` tool versions

| `type` string | Beta header | Models | Notable additions |
|---|---|---|---|
| `computer_20241022` | `computer-use-2024-10-22` | Claude 3.5 Sonnet (retired) | Baseline action set |
| `computer_20250124` | `computer-use-2025-01-24` | Sonnet 3.7, Opus 4, Sonnet 4, Opus 4.1, Sonnet 4.5, Haiku 4.5 | scroll, triple_click, left_mouse_down/up, hold_key, wait, modifier-key `text` param |
| `computer_20250429` | `computer-use-2025-01-24` (reuses) | Claude 4 family | Tool-group label only — wire type is still `computer_20250124` |
| `computer_20251124` | `computer-use-2025-11-24` | Opus 4.5, Sonnet 4.6, Opus 4.6, Opus 4.7 | `zoom` action; `enable_zoom` flag; Opus 4.7 supports 1:1 coords up to 2576 px long edge |

**Tool definition shape** (`computer_20251124`):
```json
{
  "type": "computer_20251124",
  "name": "computer",
  "display_width_px": 1024,
  "display_height_px": 768,
  "display_number": 1,
  "enable_zoom": true
}
```

### `bash` tool

Current: `bash_20250124`, `name: "bash"`. Input params: `command` (required unless `restart`), `restart` (bool). Token cost: **245 input tokens** per request.

### `text_editor` tool versions

| `type` | `name` | Notes |
|---|---|---|
| `text_editor_20241022` | `str_replace_editor` | 3.5 Sonnet (retired). Has `undo_edit`. |
| `text_editor_20250124` | `str_replace_editor` | 3.7 Sonnet (deprecated). Same caps as 20241022. |
| `text_editor_20250429` | `str_replace_based_edit_tool` | Claude 4 family. **Removed `undo_edit`.** Renamed. |
| `text_editor_20250728` | `str_replace_based_edit_tool` | Opus 4.5+, current. Adds optional `max_characters` for view truncation. |

Commands: `view`, `create`, `str_replace`, `insert`, (older only: `undo_edit`). Token cost: **700 input tokens** per request.

---

## Action space of `computer` tool

Verbatim from `computer.py` in the reference implementation:

```python
Action_20241022 = Literal[
    "key", "type", "mouse_move", "left_click", "left_click_drag",
    "right_click", "middle_click", "double_click", "screenshot",
    "cursor_position",
]
Action_20250124 = Action_20241022 | Literal[
    "left_mouse_down", "left_mouse_up", "scroll", "hold_key",
    "wait", "triple_click",
]
Action_20251124 = Action_20250124 | Literal["zoom"]
```

Per-action contract:

| Action | Coordinate | Other required | Notes |
|---|---|---|---|
| `screenshot` | — | — | Returns base64 PNG |
| `cursor_position` | — | — | Returns `"X=...,Y=..."` text |
| `mouse_move` | `[x,y]` | — | Returns screenshot |
| `left_click`, `right_click`, `middle_click`, `double_click`, `triple_click` | optional in 20250124+ (clicks at current cursor if omitted) | optional `text` (modifier) | Modifier values: `"shift"`, `"ctrl"`, `"alt"`, `"super"` |
| `left_click_drag` | `[x,y]` (end) | `start_coordinate: [x,y]` | |
| `left_mouse_down`, `left_mouse_up` | rejected | — | Move first with `mouse_move` |
| `key` | — | `text` (xdotool key syntax: `"ctrl+s"`, `"Return"`, `"Page_Down"`) | |
| `type` | — | `text` (raw text) | Chunked: 50 chars per call internally, 12 ms per keystroke |
| `scroll` | optional | `scroll_direction: "up"\|"down"\|"left"\|"right"`, `scroll_amount: int≥0` | |
| `hold_key` | — | `text`, `duration: float 0–100s` | |
| `wait` | — | `duration: float 0–100s` | |
| `zoom` (20251124, `enable_zoom: true` only) | — | `region: [x0,y0,x1,y1]` | Returns cropped image at native resolution |

---

## Coordinate system

Origin **top-left**. Pixel integers, two-element array `[x, y]`. Validation in `computer.py`: `"coordinate must be a tuple of length 2"`, `"must be a tuple of non-negative ints"`.

### Official scaling rule (paraphrased)

Anthropic's docs: the API constrains images to ~1568 px on the longest edge and ~1.15 MP total; Claude returns coordinates in that scaled space and your tool maps them back to original screen space. (Anthropic computer-use tool docs.)

Official formula:

```python
def get_scale_factor(width, height):
    long_edge = max(width, height)
    total_pixels = width * height
    long_edge_scale = 1568 / long_edge
    total_pixels_scale = math.sqrt(1_150_000 / total_pixels)
    return min(1.0, long_edge_scale, total_pixels_scale)
```

You **resize before sending** and **divide by `scale` when executing clicks** (model coords → screen coords: `screen_x = x / scale`).

### Opus 4.7 exception (paraphrased)

Per Anthropic's docs, Opus 4.7 supports up to 2576 px on the long edge with 1:1 image-pixel coordinates (no scale-factor conversion); the 1568 px guidance applies to earlier models.

### Reference implementation's actual scaling (different, more conservative)

```python
MAX_SCALING_TARGETS = {
    "XGA":   Resolution(1024, 768),   # 4:3
    "WXGA":  Resolution(1280, 800),   # 16:10
    "FWXGA": Resolution(1366, 768),   # ~16:9
}
```

Algorithm: find target whose aspect ratio is within 0.02 of source, scale to it if source is larger. Uses ImageMagick `convert {path} -resize {x}x{y}!`. **The model only sees the scaled-down dimensions** as `display_width_px/display_height_px`.

### Doc-recommended targets (paraphrased)

Anthropic's docs recommend 1024×768 or 1280×720 for general desktop tasks and 1280×800 or 1366×768 for web apps, and advise against resolutions above 1920×1080 for performance.

---

## Screenshot pipeline

Reference implementation:
1. `mkdir -p /tmp/outputs`
2. `gnome-screenshot -f <path> -p` (cursor), else `scrot -p <path>`
3. If scaling enabled: `convert <path> -resize WxH! <path>`
4. base64 encode → return as `image` content block

Cadence: **2.0 s `_screenshot_delay` before every post-action screenshot**. Triggered after every action by default (except `type` chunks: only one screenshot at end; `wait`: only post-sleep screenshot).

Token-saving: `only_n_most_recent_images` keeps last N screenshots, drops older. Disabled when prompt caching is on (cache reads are 10× cheaper than re-encoding).

Prompt-cache strategy: ephemeral cache breakpoints on last content block of 3 most recent user turns + system prompt = 4 total.

---

## Bash semantics

- One long-running `/bin/bash` per `BashTool` instance, lazy-started on first call.
- Sentinel-driven framing: `<cmd>; echo '<<exit>>'\n` — read until sentinel.
- Timeout: 120 s per command. Timeout marks session as broken, forces `restart: true`.
- Output throttle: 0.2 s between buffer polls.
- Restart: `SIGTERM` + spawn fresh.
- **Stateless on the API side.** Client maintains shell state between turns.
- No interactive commands (vim/less), no streaming, no GUI prompts.
- Output truncated at 16000 chars with `<response clipped>` + hint to use `grep -n` to find ranges.

---

## text_editor semantics

- **`view` file:** prefixed with `Here's the result of running 'cat -n' on <path>:` + line-numbered output (1-indexed, width 6 + tab).
- **`view` directory:** runs `find <path> -maxdepth 2 -not -path '*/\.*'`.
- **`view_range`:** `[start, end]`, 1-indexed inclusive, `end = -1` = EOF.
- **`create`:** fails if file exists.
- **`str_replace`:** must match **exactly once**. 0 matches → error "did not appear verbatim". >1 matches → error lists every line containing the substring. Returns 4 lines of context before/after on success.
- **`insert`:** `insert_line=0` = beginning. Validates `0 <= insert_line <= n_lines`.
- **`undo_edit`:** removed from `_20250429` onward.
- **Path validation:** must be absolute. For non-create commands, must exist. Directories: only `view`.
- **Tabs:** `expandtabs()` applied to both `old_str` and file content before matching.
- **`max_characters` (20250728+):** client-side `view` output truncation hint.

---

## Reference implementation architecture

Container = `ubuntu:22.04` + Python 3.11. Build args: `WIDTH=1024 HEIGHT=768 DISPLAY_NUM=1`.

Stack:
- Display: `Xvfb :1 -ac -screen 0 1024x768x24 -retro -dpi 96 -nolisten tcp -nolisten unix`
- WM: `mutter`
- Panel: `tint2`
- VNC: `x11vnc` :5900 + `noVNC` :6080
- UI: Streamlit :8501 + http_server.py :8080
- Input: `xdotool` (also `getmouselocation --shell`)
- Screenshot: `gnome-screenshot -p` (cursor) → fallback `scrot -p`
- Image: ImageMagick `convert`
- Apps preinstalled: Firefox-ESR, LibreOffice, gedit, xpdf, xpaint, pcmanfm, galculator, xterm

xdotool button mapping:
```python
CLICK_BUTTONS = {
    "left_click": 1, "right_click": 3, "middle_click": 2,
    "double_click": "--repeat 2 --delay 10 1",
    "triple_click": "--repeat 3 --delay 10 1",
}
# Scroll: up=4, down=5, left=6, right=7
```

Agent loop (`loop.py:sampling_loop`):
1. Pick beta flags
2. Inject cache_control on last 3 user turns + system prompt
3. Optionally drop stale screenshots
4. Call API
5. Run tools; append tool_result (text + image)
6. Loop until no tool_use blocks

Thinking blocks preserved across turns with `signature` field.

---

## Pricing notes

- System prompt overhead: 466–499 tokens auto-injected
- Computer tool definition: 735 tokens (4.x and 3.7)
- Bash tool: 245 tokens
- Text_editor tool: 700 tokens (4.x and 3.7)

---

## Known failure modes (documented or visible in reference impl)

1. **Coordinate drift** from API auto-resize if you skip your own scaling.
2. **Cursor missing** if you don't use `-p` flag.
3. **Stale state** if you skip the 2 s post-action delay.
4. **Modifier-key footgun**: `text` param on click ≠ `hold_key` ≠ `key` param.
5. **Modal dialogs** trip up the model — reference system prompt literally instructs Claude: `"When using Firefox, if a startup wizard appears, IGNORE IT. Do not even click 'skip this step'."`
6. **PDF reading** via screenshots is unreliable — system prompt tells Claude to `curl` + `pdftotext` instead.
7. **Prompt injection from screen content** — Anthropic's server-side classifier runs on prompts with computer-use tools and may force confirmation. Can opt out via support.
8. **Bash session wedges** at 120 s timeout — requires explicit `restart: true`.
9. **xdotool key syntax leaks** — `key` action's `text` field is X11 keysym names. On non-X11 you must remap.
10. **`str_replace` failure cascade** — `expandtabs()` papers over indentation but not CRLF vs LF.
11. **`create` is non-overwriting.** Must `rm` first or use `str_replace`.
12. **Scaling-target aspect-ratio gating** — odd ratios fall through to native, risking oversize and degraded accuracy.

---

## MCP relation

Per Anthropic's docs, computer use is a **client-side tool**: all screenshots, mouse/keyboard actions, and session files are captured and stored in your own environment, not by Anthropic.

Computer use is **not** an MCP server. It is a first-party Anthropic tool schema baked into the model. The execution side (xdotool/scrot/etc) lives in the developer's environment. That makes it a natural fit for an MCP server *to expose the same action surface* — but the native `computer_*` type itself cannot be transported through MCP.

A separate Anthropic API feature, `mcp_toolset` (`mcp-client-2025-11-20` beta), lets a model call remote MCP servers — that's the inverse direction, irrelevant if you're writing the server yourself.
