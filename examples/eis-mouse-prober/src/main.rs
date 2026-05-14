//! EIS mouse-click prober.
//!
//! Sibling of eis-keystroke-prober. Same EIS handoff, but emits a single
//! left-click at (x, y) via PointerAbsolute + Button capabilities.
//!
//! Usage: `cargo run --release -- <x> <y>`
//! Defaults: 400 20
//!
//! **Phase 0 spike correction discovered here:** KWin's
//! `connectToEIS(flags)` argument is a capability bit-mask, NOT a sender/
//! receiver mode bit. `1` = keyboard cap, `2` = pointer (incl.
//! PointerAbsolute/Button/Scroll), `4` = touch. The keystroke prober's
//! `flags=1` worked because it includes keyboard; for pointer we pass `2`.
//! Sender vs receiver is selected by ContextType in the handshake.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reis::{ei, event::DeviceCapability};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// /usr/include/linux/input-event-codes.h
const BTN_LEFT: u32 = 0x110;

#[zbus::proxy(
    interface = "org.kde.KWin.EIS.RemoteDesktop",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/EIS/RemoteDesktop"
)]
trait RemoteDesktop {
    #[zbus(name = "connectToEIS")]
    fn connect_to_eis(&self, flags: i32) -> zbus::Result<(zvariant::OwnedFd, i32)>;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let target_x: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(400.0);
    let target_y: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    eprintln!("target: ({target_x}, {target_y})");

    let bus = zbus::Connection::session().await.context("session bus")?;
    let proxy = RemoteDesktopProxy::new(&bus).await.context("RemoteDesktop proxy")?;
    // KWin's connectToEIS flags: 1=keyboard, 2=pointer (incl. PointerAbsolute,
    // Button, Scroll), 4=touch. We pass 2 to get a pointer-capable device.
    let (owned_fd, conn_id) = proxy.connect_to_eis(2).await.context("connectToEIS")?;
    let std_fd: std::os::fd::OwnedFd = owned_fd.into();
    let raw_fd = std_fd.into_raw_fd();
    eprintln!("got EIS fd={raw_fd} conn_id={conn_id}");

    let stream = unsafe { UnixStream::from_raw_fd(raw_fd) };
    stream.set_nonblocking(true).context("set_nonblocking")?;
    let context = ei::Context::new(stream).context("ei::Context::new")?;

    let (_connection, mut events) = context
        .handshake_tokio("kde-mcp-mouse-prober", ei::handshake::ContextType::Sender)
        .await
        .context("handshake_tokio")?;

    let mut pointer_dev: Option<reis::event::Device> = None;
    let mut last_serial: u32 = 0;
    let sequence: u32 = 0;
    eprintln!("waiting for pointer device + Resumed...");
    while let Some(result) = events.next().await {
        let event = result.context("event stream")?;
        match &event {
            reis::event::EiEvent::SeatAdded(evt) => {
                eprintln!("SeatAdded: {:?}", evt.seat.name());
                evt.seat.bind_capabilities(
                    DeviceCapability::Pointer
                        | DeviceCapability::PointerAbsolute
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                );
                context.flush().context("flush after bind")?;
            }
            reis::event::EiEvent::DeviceAdded(evt) => {
                eprintln!(
                    "DeviceAdded: name={:?} type={:?} has_pointer_abs={} has_button={}",
                    evt.device.name(),
                    evt.device.device_type(),
                    evt.device.has_capability(DeviceCapability::PointerAbsolute),
                    evt.device.has_capability(DeviceCapability::Button),
                );
                if evt.device.has_capability(DeviceCapability::PointerAbsolute)
                    && evt.device.has_capability(DeviceCapability::Button)
                {
                    pointer_dev = Some(evt.device.clone());
                }
            }
            reis::event::EiEvent::DeviceResumed(evt) => {
                eprintln!("DeviceResumed: serial={}", evt.serial);
                last_serial = evt.serial;
                if pointer_dev.is_some() {
                    break;
                }
            }
            other => eprintln!("(event) {other:?}"),
        }
    }

    let pdev = pointer_dev.context("no pointer device received")?;
    let device: &ei::Device = pdev.device();
    let pointer_abs: ei::PointerAbsolute = pdev
        .interface::<ei::PointerAbsolute>()
        .context("device has no PointerAbsolute interface")?;
    let button: ei::Button = pdev
        .interface::<ei::Button>()
        .context("device has no Button interface")?;
    eprintln!("ready. last_serial={last_serial}");

    let now_us = || -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    };

    eprintln!("start_emulating");
    device.start_emulating(last_serial, sequence);

    eprintln!("move pointer to ({target_x}, {target_y})");
    pointer_abs.motion_absolute(target_x, target_y);
    device.frame(last_serial, now_us());
    context.flush().context("flush move")?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    eprintln!("press BTN_LEFT");
    button.button(BTN_LEFT, ei::button::ButtonState::Press);
    device.frame(last_serial, now_us());
    context.flush().context("flush press")?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    eprintln!("release BTN_LEFT");
    button.button(BTN_LEFT, ei::button::ButtonState::Released);
    device.frame(last_serial, now_us());
    context.flush().context("flush release")?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    device.stop_emulating(last_serial);
    context.flush().context("flush stop_emulating")?;
    eprintln!("done — clicked ({target_x}, {target_y})");

    Ok(())
}
