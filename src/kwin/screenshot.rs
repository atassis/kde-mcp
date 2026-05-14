//! org.kde.KWin.ScreenShot2 zbus proxy.
//!
//! Each `Capture*` method takes a HashMap<&str, Variant> options dict and a
//! write-end pipe FD; KWin writes the PNG bytes to the FD. We read them out
//! on a blocking task (PNG decode happens in the caller).

use crate::error::ToolError;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use zbus::Connection;
use zvariant::Value;

#[zbus::proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
pub trait ScreenShot2 {
    fn capture_workspace(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_active_window(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_window(
        &self,
        handle: &str,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_active_screen(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_screen(
        &self,
        name: &str,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    fn capture_area(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;
}

#[derive(Clone)]
pub enum Region {
    Workspace,
    ActiveScreen,
    ActiveWindow,
    Window(String),
    Area([i32; 4]),
}

pub struct CaptureRequest {
    pub region: Region,
    pub include_cursor: bool,
}

pub async fn capture(bus: &Connection, req: CaptureRequest) -> Result<Vec<u8>, ToolError> {
    let (read_fd, write_fd) = nix::unistd::pipe().map_err(|e| ToolError::Io {
        path: "pipe()".into(),
        error: e.to_string(),
    })?;
    let read_owned: OwnedFd = read_fd;
    let write_owned = zvariant::OwnedFd::from(write_fd);

    let proxy = ScreenShot2Proxy::new(bus).await.map_err(busdc)?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("include-cursor", req.include_cursor.into());
    options.insert("native-resolution", false.into());

    let _result = match req.region {
        Region::Workspace => proxy.capture_workspace(options, write_owned).await,
        Region::ActiveScreen => proxy.capture_active_screen(options, write_owned).await,
        Region::ActiveWindow => proxy.capture_active_window(options, write_owned).await,
        Region::Window(uuid) => proxy.capture_window(&uuid, options, write_owned).await,
        Region::Area([x, y, w, h]) => {
            proxy
                .capture_area(x, y, w as u32, h as u32, options, write_owned)
                .await
        }
    }
    .map_err(busdc)?;

    let bytes = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut f = std::fs::File::from(read_owned);
        let mut buf = Vec::with_capacity(256 * 1024);
        f.read_to_end(&mut buf)?;
        Ok(buf)
    })
    .await
    .map_err(|e| ToolError::Io {
        path: "screenshot pipe".into(),
        error: e.to_string(),
    })?
    .map_err(|e| ToolError::Io {
        path: "screenshot pipe".into(),
        error: e.to_string(),
    })?;

    if bytes.is_empty() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: "screenshot pipe returned 0 bytes".into(),
        });
    }
    Ok(bytes)
}

fn busdc(e: zbus::Error) -> ToolError {
    ToolError::BusDisconnected {
        detail: e.to_string(),
    }
}
