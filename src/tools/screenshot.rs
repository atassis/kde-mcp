use crate::error::ToolError;
use crate::kwin::screenshot::{Capture, CaptureRequest, Region, capture};
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use crate::util::image::{is_terminal_class, mask_rectangles, resize_long_edge};
use base64::Engine as _;
use rmcp::model::Content;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotArgs {
    /// "active_screen" | "full" | "window:<ref>" | "element:<ref>" | [x,y,w,h]
    #[serde(default)]
    pub region: Option<ScreenshotRegion>,
    #[serde(default)]
    pub include_cursor: Option<bool>,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(untagged)]
pub enum ScreenshotRegion {
    Named(String),
    Rect([i32; 4]),
}

/// Metadata block that travels alongside the image content.
///
/// `png_base64` is intentionally omitted — the PNG goes back to the model
/// as an [`rmcp::model::Content::image`] block, not as a base64 string
/// inside a JSON tool result. The earlier shape (`{format, original_size,
/// returned_size, scale_factor, png_base64}`) forced the client to decode
/// the bytes from text, which (a) wastes context-window tokens on the
/// base64 string and (b) prevents Claude Code from routing the image
/// into the model's vision pathway. With the `Content::image` block, the
/// MCP spec's image-content type carries the bytes natively and the model
/// sees the screenshot in one pass.
#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ScreenshotMetadata {
    pub original_size: [u32; 2],
    pub returned_size: [u32; 2],
    pub scale_factor: f32,
    pub format: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn screenshot(state: &KdeMcp, args: ScreenshotArgs) -> Result<Vec<Content>, ToolError> {
    let region = resolve_region(state, args.region.as_ref()).await?;
    let include_cursor = args.include_cursor.unwrap_or(true);

    let captured = capture(
        &state.bus,
        CaptureRequest {
            region: region.clone(),
            include_cursor,
        },
    )
    .await?;

    let (terminals, mask_warnings) = list_terminal_rects(state, &region).await;
    let processed = process_image(captured, args.max_long_edge, &terminals)?;

    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&processed.png);
    let meta = ScreenshotMetadata {
        original_size: [processed.original.0, processed.original.1],
        returned_size: [processed.returned.0, processed.returned.1],
        scale_factor: processed.scale,
        format: "png".into(),
        warnings: mask_warnings,
    };
    let meta_json = serde_json::to_value(&meta).unwrap_or_else(|_| json!({}));

    // First block: the image itself, routed to the model's vision input.
    // Second block: structured metadata (size, scale, warnings) so the
    // model can reason about coordinate scaling without round-tripping.
    Ok(vec![
        Content::image(png_base64, "image/png"),
        Content::text(meta_json.to_string()),
    ])
}

async fn resolve_region(
    state: &KdeMcp,
    raw: Option<&ScreenshotRegion>,
) -> Result<Region, ToolError> {
    match raw {
        None => Ok(Region::ActiveScreen),
        Some(ScreenshotRegion::Named(s)) => match s.as_str() {
            "active_screen" => Ok(Region::ActiveScreen),
            "full" => Ok(Region::Workspace),
            s if s.starts_with("window:") => {
                let ref_id = &s["window:".len()..];
                let meta = state.refs.resolve(ref_id).await.map_err(|e| match e {
                    RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                        ref_id: ref_id.into(),
                    },
                })?;
                if meta.kind != RefKind::Window {
                    return Err(ToolError::UnsupportedRegion {
                        reason: format!("ref {ref_id} is not a window"),
                    });
                }
                Ok(Region::Window(meta.payload))
            }
            other => Err(ToolError::UnsupportedRegion {
                reason: format!("unrecognised region string: {other}"),
            }),
        },
        Some(ScreenshotRegion::Rect(r)) => Ok(Region::Area(*r)),
    }
}

/// Build the list of terminal-class window rectangles to mask in the capture.
///
/// If the user explicitly captured a single terminal window via
/// `region="window:<ref>"`, we skip masking *that* window (the user pointed
/// at it intentionally). Other terminals in the same capture are still
/// masked.
///
/// On failure (KWin script timeout, JSON parse error), we return an empty
/// rect list and a warning. Screenshot still succeeds, just unmasked — the
/// model can be informed via the response's `warnings` field.
async fn list_terminal_rects(state: &KdeMcp, captured: &Region) -> (Vec<[i32; 4]>, Vec<String>) {
    let exempt_uuid = match captured {
        Region::Window(uuid) => Some(uuid.clone()),
        _ => None,
    };
    let payload = match crate::kwin::scripting::run_script(
        &state.bus,
        crate::tools::window::JS_LIST_ALL,
        Duration::from_secs(2),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            return (
                Vec::new(),
                vec![format!(
                    "terminal masking skipped — window enumeration failed: {}",
                    e.code()
                )],
            );
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(e) => {
            return (
                Vec::new(),
                vec![format!("terminal masking skipped — bad payload: {e}")],
            );
        }
    };
    let entries = match crate::tools::window::parse_entries(&v) {
        Ok(e) => e,
        Err(e) => {
            return (
                Vec::new(),
                vec![format!(
                    "terminal masking skipped — entry parse failed: {}",
                    e.code()
                )],
            );
        }
    };
    let mut rects = Vec::new();
    for e in &entries {
        let class = match &e.class {
            Some(c) => c.text.clone(),
            None => continue,
        };
        if !is_terminal_class(&class) {
            continue;
        }
        if let Some(t) = &exempt_uuid
            && &e.uuid == t
        {
            continue;
        }
        if let Some(g) = e.geometry {
            rects.push(g);
        }
    }
    (rects, Vec::new())
}

struct ProcessedImage {
    original: (u32, u32),
    returned: (u32, u32),
    scale: f32,
    png: Vec<u8>,
}

fn process_image(
    captured: Capture,
    max_long_edge: Option<u32>,
    rects: &[[i32; 4]],
) -> Result<ProcessedImage, ToolError> {
    let img = captured.image;
    let (ow, oh) = image::GenericImageView::dimensions(&img);
    // Mask at full resolution, then resize. Cheaper for multi-monitor full
    // captures than masking-after-resize since the rect count is small.
    let masked_full = mask_rectangles(img, rects);
    let (resized, post_resize_scale) = resize_long_edge(masked_full, max_long_edge.unwrap_or(1280));
    let rw = image::GenericImageView::dimensions(&resized).0;
    let rh = image::GenericImageView::dimensions(&resized).1;
    let mut buf = Vec::with_capacity(256 * 1024);
    resized
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("png encode: {e}"),
        })?;
    // scale_factor combines KWin's device-pixel ratio (captured.scale) with
    // any further downscale applied here. The agent multiplies API coords by
    // 1/scale_factor to get screen-pixel coords for input emulation.
    let scale = post_resize_scale / captured.scale;
    Ok(ProcessedImage {
        original: (ow, oh),
        returned: (rw, rh),
        scale,
        png: buf,
    })
}
