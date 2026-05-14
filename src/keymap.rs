//! Keysym/keycode mapping plus xdotool-style chord parsing.
//!
//! Phase 2 hard-codes everything ASCII (a-z, 0-9, basic punctuation
//! whitespace) and named keys (Return, Tab, F1..F12, arrows, etc). Anything
//! outside that set is the `keyboard.type` clipboard-paste fallback's job.
//! Phase 6 polish layout-aware shifting routes through xkbcommon when
//! non-US keymaps need correct character handling.

use crate::error::ToolError;

/// evdev raw keycodes from `/usr/include/linux/input-event-codes.h`.
/// libei expects these directly; no `-8` offset.
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_LEFTALT: u32 = 56;
pub const KEY_LEFTMETA: u32 = 125;
pub const KEY_TAB: u32 = 15;
pub const KEY_ENTER: u32 = 28;
pub const KEY_ESC: u32 = 1;
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_SPACE: u32 = 57;
pub const KEY_UP: u32 = 103;
pub const KEY_LEFT: u32 = 105;
pub const KEY_RIGHT: u32 = 106;
pub const KEY_DOWN: u32 = 108;
pub const KEY_PAGEUP: u32 = 104;
pub const KEY_PAGEDOWN: u32 = 109;
pub const KEY_HOME: u32 = 102;
pub const KEY_END: u32 = 107;
pub const KEY_INSERT: u32 = 110;
pub const KEY_DELETE: u32 = 111;
pub const KEY_F1: u32 = 59;
pub const KEY_F2: u32 = 60;
pub const KEY_F3: u32 = 61;
pub const KEY_F4: u32 = 62;
pub const KEY_F5: u32 = 63;
pub const KEY_F6: u32 = 64;
pub const KEY_F7: u32 = 65;
pub const KEY_F8: u32 = 66;
pub const KEY_F9: u32 = 67;
pub const KEY_F10: u32 = 68;
pub const KEY_F11: u32 = 87;
pub const KEY_F12: u32 = 88;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl Modifier {
    pub fn keycode(self) -> u32 {
        match self {
            Modifier::Ctrl => KEY_LEFTCTRL,
            Modifier::Shift => KEY_LEFTSHIFT,
            Modifier::Alt => KEY_LEFTALT,
            Modifier::Super => KEY_LEFTMETA,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chord {
    pub modifiers: Vec<Modifier>,
    /// evdev keycode of the non-modifier key in the chord.
    pub key: u32,
}

/// Parse an xdotool-style chord string. The last `+`-separated token is the
/// key (named or single character); earlier tokens are modifiers.
pub fn parse_chord(s: &str) -> Result<Chord, ToolError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(ToolError::UnsupportedRegion {
            reason: "empty chord".into(),
        });
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("empty chord segment in {trimmed:?}"),
        });
    }
    let (key_name, mod_names) = parts.split_last().unwrap();

    let mut modifiers = Vec::with_capacity(mod_names.len());
    for m in mod_names {
        let parsed = match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "shift" => Modifier::Shift,
            "alt" => Modifier::Alt,
            "super" | "meta" | "win" => Modifier::Super,
            other => {
                return Err(ToolError::UnsupportedRegion {
                    reason: format!("unknown modifier: {other}"),
                });
            }
        };
        if modifiers.contains(&parsed) {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("modifier {m:?} appears twice"),
            });
        }
        modifiers.push(parsed);
    }

    let key = keycode_for_named_key(key_name).ok_or_else(|| ToolError::UnsupportedRegion {
        reason: format!("unknown key name: {key_name:?}"),
    })?;

    Ok(Chord { modifiers, key })
}

/// Named-key → keycode lookup. Names mirror xkb's `XKB_KEY_*` symbol set
/// but the lookup is case-insensitive on the name part. Single-character
/// names route through [`keycode_for_char`].
pub fn keycode_for_named_key(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "return" | "enter" => KEY_ENTER,
        "tab" => KEY_TAB,
        "escape" | "esc" => KEY_ESC,
        "backspace" => KEY_BACKSPACE,
        "space" => KEY_SPACE,
        "up" => KEY_UP,
        "down" => KEY_DOWN,
        "left" => KEY_LEFT,
        "right" => KEY_RIGHT,
        "page_up" | "pageup" => KEY_PAGEUP,
        "page_down" | "pagedown" => KEY_PAGEDOWN,
        "home" => KEY_HOME,
        "end" => KEY_END,
        "insert" => KEY_INSERT,
        "delete" => KEY_DELETE,
        "f1" => KEY_F1,
        "f2" => KEY_F2,
        "f3" => KEY_F3,
        "f4" => KEY_F4,
        "f5" => KEY_F5,
        "f6" => KEY_F6,
        "f7" => KEY_F7,
        "f8" => KEY_F8,
        "f9" => KEY_F9,
        "f10" => KEY_F10,
        "f11" => KEY_F11,
        "f12" => KEY_F12,
        single if single.chars().count() == 1 => {
            return keycode_for_char(single.chars().next().unwrap());
        }
        _ => return None,
    })
}

/// Resolve a printable ASCII character to its evdev keycode. Capital
/// letters share the same keycode as lowercase; the caller is responsible
/// for pressing Shift if [`shift_required`] returns true.
pub fn keycode_for_char(ch: char) -> Option<u32> {
    Some(match ch {
        '1' => 2,
        '2' => 3,
        '3' => 4,
        '4' => 5,
        '5' => 6,
        '6' => 7,
        '7' => 8,
        '8' => 9,
        '9' => 10,
        '0' => 11,
        'q' | 'Q' => 16,
        'w' | 'W' => 17,
        'e' | 'E' => 18,
        'r' | 'R' => 19,
        't' | 'T' => 20,
        'y' | 'Y' => 21,
        'u' | 'U' => 22,
        'i' | 'I' => 23,
        'o' | 'O' => 24,
        'p' | 'P' => 25,
        'a' | 'A' => 30,
        's' | 'S' => 31,
        'd' | 'D' => 32,
        'f' | 'F' => 33,
        'g' | 'G' => 34,
        'h' | 'H' => 35,
        'j' | 'J' => 36,
        'k' | 'K' => 37,
        'l' | 'L' => 38,
        'z' | 'Z' => 44,
        'x' | 'X' => 45,
        'c' | 'C' => 46,
        'v' | 'V' => 47,
        'b' | 'B' => 48,
        'n' | 'N' => 49,
        'm' | 'M' => 50,
        ' ' => KEY_SPACE,
        '\n' => KEY_ENTER,
        '\t' => KEY_TAB,
        _ => return None,
    })
}

/// True when the character needs Shift to be held down to produce it on a
/// US keymap. Over-approximate for other layouts but harmless — extra
/// modifier presses don't change behaviour for characters that don't need
/// it.
pub fn shift_required(ch: char) -> bool {
    ch.is_ascii_uppercase()
}

/// xkbcommon-based fallback for `keycode_for_char` — resolves arbitrary
/// Unicode characters via the system keymap. Returns `(keycode, needs_shift)`
/// or `None` if the keymap can't produce the character at level 0 or 1.
///
/// The keymap is rebuilt per call (~1-2ms). xkbcommon's `Keymap` isn't
/// `Sync` so we can't trivially cache it in a `static`; per-call rebuild
/// is the YAGNI path. Phase 7 polish can introduce a thread-local cache.
pub fn keycode_for_char_via_xkb(ch: char) -> Option<(u32, bool)> {
    let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let keymap = xkbcommon::xkb::Keymap::new_from_names(
        &ctx,
        "",
        "",
        "",
        "",
        None,
        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
    )?;
    let target_keysym: u32 = xkbcommon::xkb::keysym_from_name(
        &ch.to_string(),
        xkbcommon::xkb::KEYSYM_NO_FLAGS,
    )
    .raw();
    let sym = if target_keysym == xkbcommon::xkb::keysyms::KEY_NoSymbol {
        xkbcommon::xkb::utf32_to_keysym(ch as u32).raw()
    } else {
        target_keysym
    };
    if sym == xkbcommon::xkb::keysyms::KEY_NoSymbol {
        return None;
    }
    let min_kc = keymap.min_keycode().raw();
    let max_kc = keymap.max_keycode().raw();
    for raw_kc in min_kc..=max_kc {
        let kc = xkbcommon::xkb::Keycode::new(raw_kc);
        for level in 0..2 {
            let syms = keymap.key_get_syms_by_level(kc, 0, level);
            if syms.iter().any(|s| s.raw() == sym) {
                let evdev = raw_kc.saturating_sub(8);
                return Some((evdev, level == 1));
            }
        }
    }
    None
}
