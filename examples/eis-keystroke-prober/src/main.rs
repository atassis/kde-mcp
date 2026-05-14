//! EIS keystroke prober.
//!
//! Asks KWin for an EIS FD via org.kde.KWin.EIS.RemoteDesktop.connectToEIS,
//! completes the libei handshake as a Sender, waits for a keyboard device,
//! and emits Ctrl+Tab.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reis::{ei, event::DeviceCapability};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Linux evdev keycodes from /usr/include/linux/input-event-codes.h.
// libei expects evdev keycodes directly.
const KEY_LEFTCTRL: u32 = 29;
const KEY_TAB: u32 = 15;

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
    // 1. D-Bus: ask KWin for the libei FD.
    let bus = zbus::Connection::session().await.context("session bus")?;
    let proxy = RemoteDesktopProxy::new(&bus)
        .await
        .context("RemoteDesktop proxy")?;
    eprintln!("calling connectToEIS(1) — flags=1 means SENDER mode in KWin");
    let (owned_fd, conn_id) = proxy.connect_to_eis(1).await.context("connectToEIS")?;
    let std_fd: std::os::fd::OwnedFd = owned_fd.into();
    let raw_fd = std_fd.into_raw_fd();
    eprintln!("got EIS fd={raw_fd} conn_id={conn_id}");

    // 2. Wrap FD → UnixStream → ei::Context.
    let stream = unsafe { UnixStream::from_raw_fd(raw_fd) };
    stream.set_nonblocking(true).context("set_nonblocking")?;
    let context = ei::Context::new(stream).context("ei::Context::new")?;

    // 3. Handshake as Sender.
    eprintln!("handshake_tokio as Sender...");
    let (_connection, mut events) = context
        .handshake_tokio("kde-mcp-eis-prober", ei::handshake::ContextType::Sender)
        .await
        .context("handshake_tokio")?;
    eprintln!("handshake done");

    // 4. Drive the event loop until we have a Resumed keyboard device.
    let mut keyboard_dev: Option<reis::event::Device> = None;
    let mut last_serial: u32 = 0;
    let sequence: u32 = 0;
    eprintln!("waiting for keyboard device + Resumed...");
    while let Some(result) = events.next().await {
        let event = result.context("event stream")?;
        match &event {
            reis::event::EiEvent::SeatAdded(evt) => {
                eprintln!("SeatAdded: {:?}", evt.seat.name());
                evt.seat.bind_capabilities(
                    DeviceCapability::Keyboard
                        | DeviceCapability::Pointer
                        | DeviceCapability::PointerAbsolute
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                );
                context.flush().context("flush after bind")?;
            }
            reis::event::EiEvent::DeviceAdded(evt) => {
                eprintln!(
                    "DeviceAdded: name={:?} type={:?} has_kb={}",
                    evt.device.name(),
                    evt.device.device_type(),
                    evt.device.has_capability(DeviceCapability::Keyboard)
                );
                if evt.device.has_capability(DeviceCapability::Keyboard) {
                    keyboard_dev = Some(evt.device.clone());
                }
            }
            reis::event::EiEvent::DeviceResumed(evt) => {
                eprintln!("DeviceResumed: serial={}", evt.serial);
                last_serial = evt.serial;
                if keyboard_dev.is_some() {
                    break;
                }
            }
            other => {
                eprintln!("(event) {other:?}");
            }
        }
    }

    let kdev_wrap = keyboard_dev.context("no keyboard device received")?;
    let device: &ei::Device = kdev_wrap.device();
    let keyboard: ei::Keyboard = kdev_wrap
        .interface::<ei::Keyboard>()
        .context("device has no keyboard interface")?;
    eprintln!("ready. last_serial={last_serial}");

    let now_us = || -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    };

    // 5. Send Ctrl+Tab.
    eprintln!("start_emulating(last_serial={last_serial}, sequence={sequence})");
    device.start_emulating(last_serial, sequence);

    eprintln!("press Ctrl");
    keyboard.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Press);
    device.frame(last_serial, now_us());
    context.flush().context("flush ctrl press")?;
    tokio::time::sleep(Duration::from_millis(30)).await;

    eprintln!("press Tab");
    keyboard.key(KEY_TAB, ei::keyboard::KeyState::Press);
    device.frame(last_serial, now_us());
    context.flush().context("flush tab press")?;
    tokio::time::sleep(Duration::from_millis(30)).await;

    eprintln!("release Tab");
    keyboard.key(KEY_TAB, ei::keyboard::KeyState::Released);
    device.frame(last_serial, now_us());
    context.flush().context("flush tab release")?;
    tokio::time::sleep(Duration::from_millis(30)).await;

    eprintln!("release Ctrl");
    keyboard.key(KEY_LEFTCTRL, ei::keyboard::KeyState::Released);
    device.frame(last_serial, now_us());
    context.flush().context("flush ctrl release")?;
    tokio::time::sleep(Duration::from_millis(30)).await;

    device.stop_emulating(last_serial);
    context.flush().context("flush stop_emulating")?;
    eprintln!("done — Ctrl+Tab sent.");

    Ok(())
}
