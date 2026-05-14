# Phase 2 Spike Findings

Captured 2026-05-14, before Phase 2 implementation begins. Read-only investigations only.

## Summary of plan adjustments

| Source | Adjustment |
|---|---|
| Spike 0.1 (xkbcommon) | **Dropped.** The Phase 2 keymap module hard-codes ASCII keycodes and named-key names — no xkbcommon at all. The dep is removed from Task 1; Phase 6 polish re-adds it when layout-aware shifting / non-US layouts need correct handling. |
| Spike 0.2 (evdev keycodes) | All keycodes in Task 2's draft verified against `/usr/include/linux/input-event-codes.h`. No changes. |
| Spike 0.3 (wl-copy) | wl-clipboard 2.3.0 installed and working; stdin → clipboard → readback round-trip green. |

---

## 0.1 xkbcommon-rs API surface — **deferred to Phase 6**

The plan initially called for using `xkbcommon` to resolve unicode characters to keycodes. On closer reading of Task 2's `keymap.rs` draft, the implementation actually hard-codes ASCII (a-z, 0-9, basic punctuation) and falls back to clipboard-paste for everything else (>32 chars OR non-ASCII). xkbcommon isn't called anywhere.

So the dep would just be unused weight. Dropping for Phase 2.

When Phase 6 polish lands "type the right character on a non-US layout," `xkbcommon = "0.9"` re-enters the dependency tree.

---

## 0.2 evdev keycodes — verified

Probed `/usr/include/linux/input-event-codes.h` on the dev machine:

```
KEY_ESC         1
KEY_BACKSPACE   14
KEY_TAB         15
KEY_ENTER       28
KEY_LEFTCTRL    29
KEY_LEFTSHIFT   42
KEY_LEFTALT     56
KEY_SPACE       57
KEY_F12         88
KEY_HOME        102
KEY_UP          103
KEY_PAGEUP      104
KEY_END         107
KEY_DOWN        108
KEY_PAGEDOWN    109
KEY_INSERT      110
KEY_DELETE      111
KEY_LEFTMETA    125
```

All match the Phase 2 plan's `keymap.rs` constants. F1–F11 fall in 59..=88 (not all shown above but present); the plan's per-key mapping is correct.

Letters (a..z) at keycodes 16..50 in the standard layout — also verified from the same header.

---

## 0.3 wl-copy behaviour

```
$ wl-copy --version
wl-clipboard 2.3.0
$ echo -n "phase-2-spike-test" | wl-copy --type text/plain
$ wl-paste --type text/plain
phase-2-spike-test
exit=0
```

Round-trip works. Stdin pipe is the standard input mechanism (no `--content` flag). `--clear` empties the clipboard. Exit codes: 0 on success, non-zero on missing display server / missing wayland-protocols.

Decision: Task 6 spawns `wl-copy --type <mime>` with stdin piped from the tool's `content` arg. No `--paste-once` (that's a single-shot offer; we want persistent clipboard).
