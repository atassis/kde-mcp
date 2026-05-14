use crate::error::ToolError;
use crate::keymap::Modifier;
use crate::kwin::eis::{CAP_POINTER, EisSession};
use crate::server::KdeMcp;
use reis::ei;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// /usr/include/linux/input-event-codes.h
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MouseArgs {
    /// "click" | "move" | "drag" | "scroll"
    pub action: String,
    #[serde(default)]
    pub coordinate: Option<[i32; 2]>,
    #[serde(default)]
    pub start_coordinate: Option<[i32; 2]>,
    /// "left" (default) | "right" | "middle"
    #[serde(default)]
    pub button: Option<String>,
    /// 1 (default), 2 = double-click, 3 = triple-click.
    #[serde(default)]
    pub count: Option<u32>,
    /// ctrl / shift / alt / super (deferred to Phase 6 — pointer+keyboard
    /// session combination).
    #[serde(default)]
    pub modifier: Option<Vec<String>>,
    /// "up" | "down" | "left" | "right" (for action="scroll")
    #[serde(default)]
    pub scroll_direction: Option<String>,
    #[serde(default)]
    pub scroll_amount: Option<i32>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MouseResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn mouse(state: &KdeMcp, args: MouseArgs) -> Result<MouseResponse, ToolError> {
    let action = args.action.to_ascii_lowercase();
    let button = parse_button(args.button.as_deref().unwrap_or("left"))?;
    let modifiers = parse_modifiers(args.modifier.as_deref().unwrap_or(&[]))?;
    if !modifiers.is_empty() {
        return Err(ToolError::UnsupportedRegion {
            reason: "mouse modifier keys require keyboard+pointer session combine; deferred to Phase 6"
                .into(),
        });
    }

    let session = EisSession::open(&state.bus, CAP_POINTER, false, true).await?;
    let device = session.device.device().clone();
    let pointer_abs: ei::PointerAbsolute = session
        .device
        .interface::<ei::PointerAbsolute>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing PointerAbsolute interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;
    let button_iface: ei::Button = session
        .device
        .interface::<ei::Button>()
        .ok_or_else(|| ToolError::InputDispatchFailed {
            detail: "device missing Button interface".into(),
            libei_version: None,
            kwin_version: None,
        })?;
    let scroll_iface = session.device.interface::<ei::Scroll>();

    device.start_emulating(session.last_serial, 0);
    session.flush()?;

    let outcome = run_mouse_action(
        &action,
        &session,
        &device,
        &pointer_abs,
        &button_iface,
        scroll_iface.as_ref(),
        button,
        args.coordinate,
        args.start_coordinate,
        args.count.unwrap_or(1),
        args.scroll_direction.as_deref(),
        args.scroll_amount.unwrap_or(3),
    )
    .await;

    device.stop_emulating(session.last_serial);
    let _ = session.context.flush();

    outcome.map(|_| MouseResponse {
        ok: true,
        warnings: vec![],
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_mouse_action(
    action: &str,
    session: &EisSession,
    device: &ei::Device,
    pointer_abs: &ei::PointerAbsolute,
    button_iface: &ei::Button,
    scroll_iface: Option<&ei::Scroll>,
    button: u32,
    coord: Option<[i32; 2]>,
    start_coord: Option<[i32; 2]>,
    count: u32,
    scroll_dir: Option<&str>,
    scroll_amount: i32,
) -> Result<(), ToolError> {
    match action {
        "move" => {
            let [x, y] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate required for move".into(),
            })?;
            pointer_abs.motion_absolute(x as f32, y as f32);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
        }
        "click" => {
            let [x, y] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate required for click".into(),
            })?;
            pointer_abs.motion_absolute(x as f32, y as f32);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            for _ in 0..count.max(1) {
                button_iface.button(button, ei::button::ButtonState::Press);
                device.frame(session.last_serial, EisSession::now_us());
                session.flush()?;
                tokio::time::sleep(Duration::from_millis(30)).await;
                button_iface.button(button, ei::button::ButtonState::Released);
                device.frame(session.last_serial, EisSession::now_us());
                session.flush()?;
                if count > 1 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
        "drag" => {
            let [sx, sy] = start_coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "start_coordinate required for drag".into(),
            })?;
            let [ex, ey] = coord.ok_or_else(|| ToolError::UnsupportedRegion {
                reason: "coordinate (end) required for drag".into(),
            })?;
            pointer_abs.motion_absolute(sx as f32, sy as f32);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            button_iface.button(button, ei::button::ButtonState::Press);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
            tokio::time::sleep(Duration::from_millis(30)).await;
            // ~10-step linear interpolation.
            let steps = 10;
            for i in 1..=steps {
                let fx = sx as f32 + (ex - sx) as f32 * (i as f32 / steps as f32);
                let fy = sy as f32 + (ey - sy) as f32 * (i as f32 / steps as f32);
                pointer_abs.motion_absolute(fx, fy);
                device.frame(session.last_serial, EisSession::now_us());
                session.flush()?;
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            button_iface.button(button, ei::button::ButtonState::Released);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
        }
        "scroll" => {
            let scroll = scroll_iface.ok_or_else(|| ToolError::InputDispatchFailed {
                detail: "device missing Scroll interface".into(),
                libei_version: None,
                kwin_version: None,
            })?;
            if let Some([x, y]) = coord {
                pointer_abs.motion_absolute(x as f32, y as f32);
                device.frame(session.last_serial, EisSession::now_us());
                session.flush()?;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            // libei scroll uses logical-pixel units; one notch ~= 15px.
            let (dx, dy) = match scroll_dir.unwrap_or("down") {
                "up" => (0.0, -15.0 * scroll_amount as f32),
                "down" => (0.0, 15.0 * scroll_amount as f32),
                "left" => (-15.0 * scroll_amount as f32, 0.0),
                "right" => (15.0 * scroll_amount as f32, 0.0),
                other => {
                    return Err(ToolError::UnsupportedRegion {
                        reason: format!("unknown scroll_direction: {other}"),
                    });
                }
            };
            scroll.scroll(dx, dy);
            device.frame(session.last_serial, EisSession::now_us());
            session.flush()?;
        }
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unsupported mouse action: {other}"),
            });
        }
    }
    Ok(())
}

fn parse_button(name: &str) -> Result<u32, ToolError> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "left" => BTN_LEFT,
        "right" => BTN_RIGHT,
        "middle" => BTN_MIDDLE,
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unknown button: {other}"),
            });
        }
    })
}

fn parse_modifiers(names: &[String]) -> Result<Vec<Modifier>, ToolError> {
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let m = match n.to_ascii_lowercase().as_str() {
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
        out.push(m);
    }
    Ok(out)
}
