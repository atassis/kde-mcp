//! Virtual-mode bootstrap.
//!
//! Spawns a private KDE Plasma 6 / Wayland session in child processes:
//!
//!   1. `dbus-launch --sh-syntax` — creates an isolated session bus and
//!      exports the address into our env.
//!   2. `kwin_wayland --virtual --width 1920 --height 1080` — compositor
//!      inside the new bus.
//!   3. `at-spi-bus-launcher --launch-immediately` — the AT-SPI bus so
//!      `inspect` / `observe` work inside the virtual session.
//!   4. `wayvnc` on the configured port (default 5910).
//!
//! Each child gets its own session via `setsid()` so SIGTERM to our group
//! doesn't kill them; we kill them explicitly on shutdown.
//!
//! **Notes:**
//! - The first-cut treats every child spawn that fails (binary missing,
//!   etc.) as a degraded-state warning — the server still starts, but
//!   tools that need that child fail at call time.
//! - The shutdown path is wired but only invoked on process exit via the
//!   `HANDLE` `OnceLock`. SIGTERM-from-systemd path is Phase 6 polish.

use super::BootstrappedState;
use crate::cli::Mode;
use crate::error::ToolError;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;
use zbus::Connection;

pub async fn bootstrap() -> Result<BootstrappedState, ToolError> {
    let handle = VirtualHandle::spawn().await?;
    let bus = Connection::session()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: format!("virtual inner bus: {e}"),
        })?;
    let _ = HANDLE.set(handle);
    Ok(BootstrappedState {
        mode: Mode::Virtual,
        bus,
    })
}

static HANDLE: OnceLock<VirtualHandle> = OnceLock::new();

pub struct VirtualHandle {
    pids: Vec<i32>,
    #[allow(dead_code)]
    bus_address: String,
}

impl VirtualHandle {
    async fn spawn() -> Result<Self, ToolError> {
        // 1. dbus-launch produces a fresh session bus. We export its address
        //    into our env before spawning anything.
        let launch = Command::new("dbus-launch")
            .arg("--sh-syntax")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("dbus-launch: {e}"),
            })?;
        if !launch.status.success() {
            return Err(ToolError::ExternalCommandFailed {
                stderr: String::from_utf8_lossy(&launch.stderr).to_string(),
            });
        }
        let out = String::from_utf8_lossy(&launch.stdout);
        let bus_address = parse_dbus_address(&out).ok_or_else(|| {
            ToolError::ExternalCommandFailed {
                stderr: format!("could not parse DBUS_SESSION_BUS_ADDRESS from: {out}"),
            }
        })?;
        // SAFETY: env mutation happens before the tokio runtime hands tool
        // calls to threads that read this var. The tokio runtime is alive
        // but no tool has fired yet during bootstrap.
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &bus_address);
        }

        let mut pids = Vec::new();

        // 2. kwin_wayland in headless mode.
        let kwin = spawn_child(
            "kwin_wayland",
            &[
                "--virtual",
                "--width",
                "1920",
                "--height",
                "1080",
                "--xwayland",
            ],
            &bus_address,
        )?;
        pids.push(kwin);

        tokio::time::sleep(Duration::from_millis(500)).await;

        // 3. AT-SPI bus launcher.
        match spawn_child(
            "at-spi-bus-launcher",
            &["--launch-immediately"],
            &bus_address,
        ) {
            Ok(p) => pids.push(p),
            Err(e) => {
                tracing::warn!(error=%e.code(),
                    "at-spi-bus-launcher failed to spawn — inspect/observe will be limited");
            }
        }

        // 4. wayvnc on the configured port (CLI populates KDE_MCP_VNC_PORT).
        let vnc_port = std::env::var("KDE_MCP_VNC_PORT").unwrap_or_else(|_| "5910".into());
        match spawn_child("wayvnc", &["127.0.0.1", &vnc_port], &bus_address) {
            Ok(p) => pids.push(p),
            Err(e) => {
                tracing::warn!(error=%e.code(),
                    "wayvnc failed to spawn — virtual session has no visual readback");
            }
        }

        Ok(VirtualHandle { pids, bus_address })
    }

    pub fn shutdown(self) {
        for pid in &self.pids {
            unsafe {
                libc::kill(*pid, libc::SIGTERM);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
        for pid in &self.pids {
            unsafe {
                libc::kill(*pid, libc::SIGKILL);
            }
        }
    }
}

fn spawn_child(program: &str, args: &[&str], bus_address: &str) -> Result<i32, ToolError> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.env("DBUS_SESSION_BUS_ADDRESS", bus_address);
    cmd.env("WAYLAND_DISPLAY", "wayland-virtual");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    // SAFETY: pre_exec runs in the forked child between fork and exec.
    // setsid is signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let child = cmd.spawn().map_err(|e| ToolError::ExternalCommandFailed {
        stderr: format!("spawn {program}: {e}"),
    })?;
    Ok(child.id() as i32)
}

fn parse_dbus_address(sh_output: &str) -> Option<String> {
    for line in sh_output.lines() {
        if let Some(rest) = line.strip_prefix("DBUS_SESSION_BUS_ADDRESS=") {
            let cleaned = rest
                .trim_end_matches(';')
                .trim_matches('\'')
                .trim_matches('"');
            return Some(cleaned.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dbus_launch_sh_syntax() {
        let sample = "\
DBUS_SESSION_BUS_ADDRESS='unix:abstract=/tmp/dbus-abc123,guid=def456';
export DBUS_SESSION_BUS_ADDRESS;
DBUS_SESSION_BUS_PID=12345;
";
        let addr = parse_dbus_address(sample).unwrap();
        assert_eq!(addr, "unix:abstract=/tmp/dbus-abc123,guid=def456");
    }

    #[test]
    fn missing_address_returns_none() {
        assert!(parse_dbus_address("DBUS_SESSION_BUS_PID=42").is_none());
    }
}
