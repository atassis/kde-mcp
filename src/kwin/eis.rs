//! Per-call EIS session helper.
//!
//! Wraps the libei handshake the two probers pioneered (see
//! `examples/eis-keystroke-prober/` and `examples/eis-mouse-prober/`).
//!
//! ## Send isolation
//!
//! reis's `EiConvertEventStream` holds a `dyn FnOnce(u64)` callback that's
//! not `Send`, so the handshake-and-await-resume loop can't run on the
//! multi-threaded tokio runtime that backs `rmcp`'s tool dispatch. We
//! isolate the !Send work inside [`tokio::task::spawn_blocking`] + a fresh
//! current-thread runtime; only the Send members of the session
//! (`ei::Context`, `reis::event::Device`, `u32`) cross the boundary back.
//! After `open` returns, the rest of the tool body uses sync methods on
//! the device / context — no further awaits on !Send state.

use crate::error::ToolError;
use futures_util::StreamExt;
use reis::enumflags2;
use reis::{ei, event::DeviceCapability};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zbus::Connection;

/// KWin's `connectToEIS(flags)` argument is a **capability bit-mask**, not
/// a mode bit. Verified empirically while building the mouse prober — the
/// Phase 0 spike postscript's "SENDER mode" reading was wrong. See
/// `docs/research/2026-05-14-phase-0-spikes.md` for the correction.
pub const CAP_KEYBOARD: i32 = 1;
pub const CAP_POINTER: i32 = 2;
#[allow(dead_code)] // Touch tools are not in Phase 2 scope.
pub const CAP_TOUCH: i32 = 4;

#[zbus::proxy(
    interface = "org.kde.KWin.EIS.RemoteDesktop",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/EIS/RemoteDesktop"
)]
trait RemoteDesktop {
    #[zbus(name = "connectToEIS")]
    fn connect_to_eis(&self, flags: i32) -> zbus::Result<(zvariant::OwnedFd, i32)>;
}

pub struct EisSession {
    pub context: ei::Context,
    pub device: reis::event::Device,
    /// Set when the session was opened with both keyboard and pointer caps
    /// and KWin produced separate devices for each. The primary `device`
    /// above is the *pointer*-bearing device in the combined case; this
    /// field holds the keyboard sibling. Used by `mouse modifier+click`.
    pub keyboard_device: Option<reis::event::Device>,
    pub last_serial: u32,
}

impl EisSession {
    /// Open an EIS session and drive the handshake until a matching device
    /// is resumed. `caps_flags` is OR'd from [`CAP_KEYBOARD`] /
    /// [`CAP_POINTER`] / [`CAP_TOUCH`]. `expect_keyboard` / `expect_pointer`
    /// are predicates the returned device must satisfy.
    pub async fn open(
        bus: &Connection,
        caps_flags: i32,
        expect_keyboard: bool,
        expect_pointer: bool,
    ) -> Result<Self, ToolError> {
        // D-Bus call on the calling runtime (it's Send).
        let proxy = RemoteDesktopProxy::new(bus)
            .await
            .map_err(|e| ToolError::BusDisconnected {
                detail: e.to_string(),
            })?;
        let (owned_fd, _conn_id) =
            proxy
                .connect_to_eis(caps_flags)
                .await
                .map_err(|e| ToolError::BusDisconnected {
                    detail: e.to_string(),
                })?;
        let std_fd: std::os::fd::OwnedFd = owned_fd.into();
        let raw_fd = std_fd.into_raw_fd();

        // The libei handshake holds !Send state. Drive it on a dedicated
        // current-thread runtime via spawn_blocking. The session pieces we
        // pass back are Send.
        let join = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| ToolError::Io {
                    path: "tokio current_thread rt".into(),
                    error: e.to_string(),
                })?;
            rt.block_on(handshake_and_wait_for_device(
                raw_fd,
                expect_keyboard,
                expect_pointer,
            ))
        })
        .await
        .map_err(|e| ToolError::InputDispatchFailed {
            detail: format!("spawn_blocking join: {e}"),
            libei_version: None,
            kwin_version: None,
        })?;

        let (context, device, keyboard_device, last_serial) = join?;
        Ok(EisSession {
            context,
            device,
            keyboard_device,
            last_serial,
        })
    }

    pub fn now_us() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }

    /// Flush the libei context, mapping errors to `ToolError`.
    pub fn flush(&self) -> Result<(), ToolError> {
        self.context
            .flush()
            .map_err(|e| ToolError::InputDispatchFailed {
                detail: format!("flush: {e}"),
                libei_version: None,
                kwin_version: None,
            })
    }
}

async fn handshake_and_wait_for_device(
    raw_fd: std::os::fd::RawFd,
    expect_keyboard: bool,
    expect_pointer: bool,
) -> Result<
    (
        ei::Context,
        reis::event::Device,
        Option<reis::event::Device>,
        u32,
    ),
    ToolError,
> {
    let stream = unsafe { UnixStream::from_raw_fd(raw_fd) };
    stream.set_nonblocking(true).map_err(|e| ToolError::Io {
        path: "eis stream".into(),
        error: e.to_string(),
    })?;
    let context = ei::Context::new(stream).map_err(|e| ToolError::Io {
        path: "ei::Context".into(),
        error: e.to_string(),
    })?;

    let (_conn, mut events) = context
        .handshake_tokio("kde-mcp", ei::handshake::ContextType::Sender)
        .await
        .map_err(|e| ToolError::InputDispatchFailed {
            detail: format!("handshake: {e}"),
            libei_version: None,
            kwin_version: None,
        })?;

    let mut chosen_dev: Option<reis::event::Device> = None;
    let mut keyboard_aux: Option<reis::event::Device> = None;
    let mut last_serial: u32 = 0;
    // Combined session: we expect TWO DeviceAdded events. Keep collecting
    // until both arrive (or timeout). Single-cap sessions break on first
    // matching Resumed as before.
    let combined = expect_keyboard && expect_pointer;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let next = tokio::time::timeout(remaining, events.next()).await;
        let Ok(Some(result)) = next else {
            break;
        };
        let event = result.map_err(|e| ToolError::InputDispatchFailed {
            detail: format!("event stream: {e}"),
            libei_version: None,
            kwin_version: None,
        })?;
        match &event {
            reis::event::EiEvent::SeatAdded(evt) => {
                let mut bind: enumflags2::BitFlags<DeviceCapability> =
                    DeviceCapability::Scroll.into();
                if expect_keyboard {
                    bind |= DeviceCapability::Keyboard;
                }
                if expect_pointer {
                    bind |= DeviceCapability::Pointer
                        | DeviceCapability::PointerAbsolute
                        | DeviceCapability::Button;
                }
                evt.seat.bind_capabilities(bind);
                context
                    .flush()
                    .map_err(|e| ToolError::InputDispatchFailed {
                        detail: format!("flush after bind: {e}"),
                        libei_version: None,
                        kwin_version: None,
                    })?;
            }
            reis::event::EiEvent::DeviceAdded(evt) => {
                let has_kb = evt.device.has_capability(DeviceCapability::Keyboard);
                let has_ptr = evt.device.has_capability(DeviceCapability::PointerAbsolute)
                    && evt.device.has_capability(DeviceCapability::Button);
                if combined {
                    // Pointer device is the primary; keyboard is the aux.
                    if has_ptr && chosen_dev.is_none() {
                        chosen_dev = Some(evt.device.clone());
                    } else if has_kb && keyboard_aux.is_none() {
                        keyboard_aux = Some(evt.device.clone());
                    }
                } else {
                    let match_kb = expect_keyboard && has_kb;
                    let match_ptr = expect_pointer && has_ptr;
                    if match_kb || match_ptr {
                        chosen_dev = Some(evt.device.clone());
                    }
                }
            }
            reis::event::EiEvent::DeviceResumed(evt) => {
                last_serial = evt.serial;
                let ready = if combined {
                    chosen_dev.is_some() && keyboard_aux.is_some()
                } else {
                    chosen_dev.is_some()
                };
                if ready {
                    break;
                }
            }
            _ => {}
        }
    }

    let device = chosen_dev.ok_or_else(|| ToolError::InputDispatchFailed {
        detail: "no matching device materialised within 3s".into(),
        libei_version: None,
        kwin_version: None,
    })?;
    Ok((context, device, keyboard_aux, last_serial))
}
