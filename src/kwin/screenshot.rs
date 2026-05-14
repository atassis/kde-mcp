//! org.kde.KWin.ScreenShot2 zbus proxy.
//!
//! Each `Capture*` method takes a HashMap<&str, Variant> options dict and a
//! write-end pipe FD. KWin writes **raw ARGB32 pixel data** (not PNG) to the
//! FD and returns a results dict carrying `width`, `height`, `stride`,
//! `format` (Qt's QImage::Format enum as u32), and `scale`. We read the
//! pipe concurrently with the D-Bus call (the pixel stream can be tens of
//! MB and won't fit in the kernel pipe buffer), then assemble a
//! `DynamicImage` for the caller to mask + resize + encode as PNG.

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
    #[zbus(name = "CaptureWorkspace")]
    fn capture_workspace(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    #[zbus(name = "CaptureActiveWindow")]
    fn capture_active_window(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    #[zbus(name = "CaptureWindow")]
    fn capture_window(
        &self,
        handle: &str,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    #[zbus(name = "CaptureActiveScreen")]
    fn capture_active_screen(
        &self,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    #[zbus(name = "CaptureScreen")]
    fn capture_screen(
        &self,
        name: &str,
        options: HashMap<&str, zvariant::Value<'_>>,
        pipe: zvariant::OwnedFd,
    ) -> zbus::Result<HashMap<String, zvariant::OwnedValue>>;

    #[zbus(name = "CaptureArea")]
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

/// A captured image as a `DynamicImage` ready for masking / resize / encode.
pub struct Capture {
    pub image: image::DynamicImage,
    pub scale: f32,
}

pub async fn capture(bus: &Connection, req: CaptureRequest) -> Result<Capture, ToolError> {
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

    // Start reading the pipe BEFORE the D-Bus call returns — KWin streams
    // pixels through the FD and a multi-MB capture won't fit in the default
    // 64KB pipe buffer. Both halves run concurrently.
    let reader = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut f = std::fs::File::from(read_owned);
        let mut buf = Vec::with_capacity(8 * 1024 * 1024);
        f.read_to_end(&mut buf)?;
        Ok(buf)
    });

    let results = match req.region {
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

    let bytes = reader
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

    let width = read_u32(&results, "width")?;
    let height = read_u32(&results, "height")?;
    let stride = read_u32(&results, "stride")?;
    let qfmt = read_u32(&results, "format")?;
    let scale = read_f64(&results, "scale").unwrap_or(1.0) as f32;

    let image = decode_qimage(&bytes, width, height, stride, qfmt)?;
    Ok(Capture { image, scale })
}

fn read_u32(map: &HashMap<String, zvariant::OwnedValue>, key: &str) -> Result<u32, ToolError> {
    map.get(key)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| ToolError::ExternalCommandFailed {
            stderr: format!("screenshot result missing/invalid u32 field: {key}"),
        })
}

fn read_f64(map: &HashMap<String, zvariant::OwnedValue>, key: &str) -> Option<f64> {
    map.get(key).and_then(|v| f64::try_from(v).ok())
}

/// Convert raw QImage pixel data to a `DynamicImage`.
///
/// Qt's `QImage::Format` enum values (Qt 6):
///   3  Indexed8
///   4  RGB32                = 0xffRRGGBB → little-endian byte order BGRX
///   5  ARGB32               = 0xAARRGGBB → little-endian byte order BGRA
///   6  ARGB32_Premultiplied = same byte order; alpha is pre-multiplied into RGB
///   17 RGBA8888             = byte order RGBA (no conversion needed)
///   18 RGBA8888_Premultiplied
///
/// In practice KWin emits format 4, 5, or 6 on Linux. We swap channels for
/// those; for 17/18 we use the bytes directly. Any other format → error.
fn decode_qimage(
    bytes: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    qfmt: u32,
) -> Result<image::DynamicImage, ToolError> {
    if width == 0 || height == 0 {
        return Err(ToolError::ExternalCommandFailed {
            stderr: "screenshot zero dimensions".into(),
        });
    }
    let row_bytes = (width as usize) * 4;
    let stride = stride as usize;
    let h = height as usize;
    let required = stride.saturating_mul(h);
    if bytes.len() < required {
        return Err(ToolError::ExternalCommandFailed {
            stderr: format!(
                "screenshot bytes too short: got {}, need {} (stride={}, h={})",
                bytes.len(),
                required,
                stride,
                h
            ),
        });
    }

    let mut out: Vec<u8> = Vec::with_capacity(row_bytes * h);
    match qfmt {
        // BGRA-byte-order, alpha=ignored (RGB32) or used as-is (ARGB32) or
        // un-premultiplied (ARGB32_Premultiplied). We do *not* attempt to
        // un-premultiply in Phase 1; the visual difference is small for the
        // agent's purposes, and the masking + downscale that follows hides
        // most artifacts.
        4..=6 => {
            for row in 0..h {
                let line = &bytes[row * stride..row * stride + row_bytes];
                for px in line.chunks_exact(4) {
                    let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
                    let a = if qfmt == 4 { 255 } else { a };
                    out.extend_from_slice(&[r, g, b, a]);
                }
            }
        }
        17..=18 => {
            for row in 0..h {
                let line = &bytes[row * stride..row * stride + row_bytes];
                out.extend_from_slice(line);
            }
        }
        other => {
            return Err(ToolError::ExternalCommandFailed {
                stderr: format!(
                    "screenshot has unsupported QImage::Format {other}; expected 4/5/6/17/18"
                ),
            });
        }
    }

    let buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, out)
        .ok_or_else(|| ToolError::ExternalCommandFailed {
            stderr: "image buffer construction failed".into(),
        })?;
    Ok(image::DynamicImage::ImageRgba8(buf))
}

fn busdc(e: zbus::Error) -> ToolError {
    ToolError::BusDisconnected {
        detail: e.to_string(),
    }
}
