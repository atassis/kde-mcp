use crate::error::ToolError;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardReadArgs {
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardReadResponse {
    pub mime_type: String,
    pub encoding: String,
    pub content: Tagged<String>,
    pub origin: String,
    pub available_types: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
pub struct ClipboardWriteArgs {
    pub content: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ClipboardWriteResponse {}

pub async fn clipboard_read(
    _state: &KdeMcp,
    args: ClipboardReadArgs,
) -> Result<ClipboardReadResponse, ToolError> {
    let requested_mime = args.mime_type.unwrap_or_else(|| "text/plain".into());
    let encoding = args.encoding.unwrap_or_else(|| "utf8".into());
    let types = list_types().await?;
    if !types.iter().any(|t| t == &requested_mime) {
        return Err(ToolError::TargetNotFound {
            closest_match: closest(&types, &requested_mime),
        });
    }
    let bytes = paste_bytes(&requested_mime).await?;
    let (content_str, encoding_used) = match encoding.as_str() {
        "utf8" => (
            String::from_utf8(bytes).map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("utf8 decode: {e}"),
            })?,
            "utf8".to_string(),
        ),
        "base64" => (
            base64::engine::general_purpose::STANDARD.encode(&bytes),
            "base64".to_string(),
        ),
        other => {
            return Err(ToolError::UnsupportedRegion {
                reason: format!("unsupported encoding: {other}"),
            });
        }
    };
    Ok(ClipboardReadResponse {
        mime_type: requested_mime,
        encoding: encoding_used,
        content: Tagged {
            trust: Trust::External,
            text: content_str,
        },
        origin: "external".to_string(),
        available_types: types,
    })
}

async fn list_types() -> Result<Vec<String>, ToolError> {
    let out = tokio::process::Command::new("wl-paste")
        .arg("--list-types")
        .output()
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawning wl-paste: {e}"),
        })?;
    if !out.status.success() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    Ok(out
        .stdout
        .split(|b| *b == b'\n')
        .map(|line| String::from_utf8_lossy(line).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

async fn paste_bytes(mime: &str) -> Result<Vec<u8>, ToolError> {
    let out = tokio::process::Command::new("wl-paste")
        .arg("--no-newline")
        .arg("--type")
        .arg(mime)
        .output()
        .await
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawning wl-paste: {e}"),
        })?;
    if !out.status.success() {
        return Err(ToolError::ExternalCommandFailed {
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    Ok(out.stdout)
}

fn closest(types: &[String], want: &str) -> Option<String> {
    types.iter().min_by_key(|t| levenshtein(t, want)).cloned()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (cur[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub async fn clipboard_write(
    _state: &KdeMcp,
    _args: ClipboardWriteArgs,
) -> Result<ClipboardWriteResponse, ToolError> {
    Err(ToolError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("text/plain", "text/plain"), 0);
    }

    #[test]
    fn closest_picks_nearest_mime() {
        let types = vec![
            "text/plain".to_string(),
            "image/png".to_string(),
            "text/html".to_string(),
        ];
        // "text/plane" → "text/plain" wins (distance 1) over "text/html" (distance 5).
        assert_eq!(closest(&types, "text/plane"), Some("text/plain".into()));
    }
}
