use crate::error::ToolError;
use crate::kwin::screenshot::{CaptureRequest, Region, capture};
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadTextArgs {
    /// Same region grammar as `screenshot`: "active_screen" | "full" |
    /// "window:<ref>" | "element:<ref>" | [x,y,w,h].
    pub region: serde_json::Value,
    /// Tesseract language codes joined with `+`, e.g. "eng+rus".
    /// Defaults to "eng".
    #[serde(default)]
    pub lang: Option<String>,
    /// If true, the response includes per-word bounding boxes.
    #[serde(default)]
    pub include_bboxes: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReadTextResponse {
    pub text: Tagged<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<RecognizedWord>>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RecognizedWord {
    pub text: String,
    pub bbox: [i32; 4],
    pub confidence: f32,
}

pub async fn read_text(state: &KdeMcp, args: ReadTextArgs) -> Result<ReadTextResponse, ToolError> {
    // Resolve region the same way screenshot does.
    let region = resolve_region(state, &args.region).await?;
    let captured = capture(
        &state.bus,
        CaptureRequest {
            region,
            include_cursor: false,
        },
    )
    .await?;

    let lang = args.lang.unwrap_or_else(|| "eng".to_string());
    let want_bboxes = args.include_bboxes.unwrap_or(false);

    // tesseract crate is sync + does C-FFI; run on a blocking thread.
    let img = captured.image;
    let ocr_result = tokio::task::spawn_blocking(move || run_ocr(img, &lang, want_bboxes))
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("ocr join: {e}"),
        })??;
    Ok(ReadTextResponse {
        text: Tagged {
            trust: Trust::External,
            text: ocr_result.text,
        },
        words: ocr_result.words,
    })
}

struct OcrOutcome {
    text: String,
    words: Option<Vec<RecognizedWord>>,
}

fn run_ocr(img: image::DynamicImage, lang: &str, want_bboxes: bool) -> Result<OcrOutcome, ToolError> {
    // Re-encode to PNG in memory; tesseract's set_image_from_mem accepts
    // any image format. We could also feed raw RGBA bytes via set_frame
    // but the PNG path is simpler and copes with the image crate's own
    // colour-space handling.
    let mut buf = Vec::with_capacity(256 * 1024);
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("ocr png encode: {e}"),
        })?;

    let mut ts = tesseract::Tesseract::new(None, Some(lang))
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("tesseract init lang={lang}: {e}"),
        })?
        .set_image_from_mem(&buf)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("tesseract set_image: {e}"),
        })?;
    let text = ts
        .get_text()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("tesseract get_text: {e}"),
        })?;
    let words = if want_bboxes {
        Some(extract_words(&mut ts)?)
    } else {
        None
    };
    Ok(OcrOutcome { text, words })
}

fn extract_words(ts: &mut tesseract::Tesseract) -> Result<Vec<RecognizedWord>, ToolError> {
    // tesseract 0.15's ResultIterator API exposes per-word geometry +
    // confidence. The crate doesn't yet expose all the page-iterator
    // operations we'd want; we approximate by parsing the TSV output.
    let tsv = ts
        .get_tsv_text(0)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("tesseract tsv: {e}"),
        })?;
    let mut out = Vec::new();
    // TSV columns (tesseract 5): level page_num block_num par_num line_num
    //   word_num left top width height conf text
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        if cols[0] != "5" {
            continue; // level=5 is per-word
        }
        let left = cols[6].parse::<i32>().unwrap_or(0);
        let top = cols[7].parse::<i32>().unwrap_or(0);
        let width = cols[8].parse::<i32>().unwrap_or(0);
        let height = cols[9].parse::<i32>().unwrap_or(0);
        let conf = cols[10].parse::<f32>().unwrap_or(-1.0);
        let text = cols[11].trim().to_string();
        if text.is_empty() || conf < 0.0 {
            continue;
        }
        out.push(RecognizedWord {
            text,
            bbox: [left, top, width, height],
            confidence: conf,
        });
    }
    Ok(out)
}

async fn resolve_region(
    state: &KdeMcp,
    raw: &serde_json::Value,
) -> Result<Region, ToolError> {
    if let Some(s) = raw.as_str() {
        match s {
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
                reason: format!("unrecognised region: {other}"),
            }),
        }
    } else if let Some(arr) = raw.as_array() {
        if arr.len() != 4 {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("rect region must be [x,y,w,h], got {} elements", arr.len()),
            });
        }
        Ok(Region::Area([
            arr[0].as_i64().unwrap_or(0) as i32,
            arr[1].as_i64().unwrap_or(0) as i32,
            arr[2].as_i64().unwrap_or(0) as i32,
            arr[3].as_i64().unwrap_or(0) as i32,
        ]))
    } else {
        Err(ToolError::UnsupportedRegion {
            reason: "region must be a string or [x,y,w,h]".into(),
        })
    }
}
