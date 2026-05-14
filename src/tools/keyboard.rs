use crate::error::ToolError;
use crate::keymap::{KEY_LEFTCTRL, KEY_LEFTSHIFT, keycode_for_char, parse_chord, shift_required};
use crate::kwin::eis::{CAP_KEYBOARD, EisSession};
use crate::server::KdeMcp;
use reis::ei;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// Split per spec §4.5 poka-yoke: type accepts text only; key accepts keys
// only. "Type while holding Ctrl" can only be expressed as two sequential
// calls, which is what we want.

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardTypeArgs {
    pub text: String,
    /// "auto" (default) | "always" | "never". Auto routes to clipboard-paste
    /// when text > 32 chars or contains non-ASCII characters.
    #[serde(default)]
    pub use_clipboard: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardKeyArgs {
    pub keys: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KeyboardResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_clipboard: Option<bool>,
}

pub async fn keyboard_type(
    state: &KdeMcp,
    args: KeyboardTypeArgs,
) -> Result<KeyboardResponse, ToolError> {
    let mode = args.use_clipboard.as_deref().unwrap_or("auto");
    let needs_clipboard = match mode {
        "always" => true,
        "never" => false,
        _ => args.text.chars().count() > 32 || !args.text.is_ascii(),
    };

    if needs_clipboard {
        return type_via_clipboard(state, &args.text).await;
    }

    let session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session.device.interface::<ei::Keyboard>().ok_or_else(|| {
        ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        }
    })?;

    device.start_emulating(session.last_serial, 0);
    session.flush()?;

    for ch in args.text.chars() {
        let keycode = keycode_for_char(ch).ok_or_else(|| ToolError::UnsupportedRegion {
            reason: format!(
                "cannot type character {ch:?} via direct keymap; set use_clipboard=always"
            ),
        })?;
        let shift = shift_required(ch);
        if shift {
            kb.key(KEY_LEFTSHIFT, ei::keyboard::KeyState::Press);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
        }
        kb.key(keycode, ei::keyboard::KeyState::Press);
        device.frame(session.last_serial, EisSession::now_us());
        session.flush()?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        kb.key(keycode, ei::keyboard::KeyState::Released);
        device.frame(session.last_serial, EisSession::now_us());
        session.flush()?;
        if shift {
            kb.key(KEY_LEFTSHIFT, ei::keyboard::KeyState::Released);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();
    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: Some(false),
    })
}

pub async fn keyboard_key(
    state: &KdeMcp,
    args: KeyboardKeyArgs,
) -> Result<KeyboardResponse, ToolError> {
    let chord = parse_chord(&args.keys)?;

    let session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session.device.interface::<ei::Keyboard>().ok_or_else(|| {
        ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        }
    })?;

    device.start_emulating(session.last_serial, 0);
    session.flush()?;

    // Press modifiers in order, key, release in reverse.
    for m in &chord.modifiers {
        kb.key(m.keycode(), ei::keyboard::KeyState::Press);
        device.frame(session.last_serial, EisSession::now_us());
        session.flush()?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    kb.key(chord.key, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    tokio::time::sleep(Duration::from_millis(30)).await;
    kb.key(chord.key, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    for m in chord.modifiers.iter().rev() {
        tokio::time::sleep(Duration::from_millis(10)).await;
        kb.key(m.keycode(), ei::keyboard::KeyState::Released);
        device.frame(session.last_serial, EisSession::now_us());
        session.flush()?;
    }

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();
    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: None,
    })
}

/// Save current clipboard, set our text on it, send Ctrl+V, restore the
/// prior clipboard. Used by `keyboard.type` when text contains non-ASCII or
/// exceeds 32 chars (spec §6.2 use_clipboard "auto" rule).
async fn type_via_clipboard(state: &KdeMcp, text: &str) -> Result<KeyboardResponse, ToolError> {
    use tokio::io::AsyncWriteExt;

    // Save existing clipboard (best-effort).
    let saved = tokio::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .await
        .ok();

    // Write our text.
    let mut child = tokio::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/plain")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawn wl-copy: {e}"),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("write to wl-copy: {e}"),
            })?;
        drop(stdin);
    }
    let _ = child.wait().await;

    // Send Ctrl+V.
    let session = EisSession::open(&state.bus, CAP_KEYBOARD, true, false).await?;
    let device = session.device.device().clone();
    let kb: ei::Keyboard = session.device.interface::<ei::Keyboard>().ok_or_else(|| {
        ToolError::InputDispatchFailed {
            detail: "device missing Keyboard interface".into(),
            libei_version: None,
            kwin_version: None,
        }
    })?;
    let paste_key = keycode_for_char('v').unwrap();
    device.start_emulating(session.last_serial, 0);
    session.flush()?;
    kb.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    tokio::time::sleep(Duration::from_millis(15)).await;
    kb.key(paste_key, ei::keyboard::KeyState::Press);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    tokio::time::sleep(Duration::from_millis(30)).await;
    kb.key(paste_key, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    tokio::time::sleep(Duration::from_millis(15)).await;
    kb.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Released);
    device.frame(session.last_serial, EisSession::now_us());
    session.flush()?;
    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();

    // Give the paste a beat to be consumed by the focused app.
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Restore prior clipboard (best-effort).
    if let Some(prior) = saved
        && prior.status.success()
        && !prior.stdout.is_empty()
    {
        let mut restore = tokio::process::Command::new("wl-copy")
            .arg("--type")
            .arg("text/plain")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .ok();
        if let Some(child) = restore.as_mut()
            && let Some(mut stdin) = child.stdin.take()
        {
            let _ = stdin.write_all(&prior.stdout).await;
        }
        if let Some(mut child) = restore {
            let _ = child.wait().await;
        }
    }

    Ok(KeyboardResponse {
        ok: true,
        warnings: vec![],
        used_clipboard: Some(true),
    })
}
