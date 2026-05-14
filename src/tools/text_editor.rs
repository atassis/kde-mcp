use crate::error::ToolError;
use crate::server::KdeMcp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorViewArgs {
    pub path: String,
    #[serde(default)]
    pub view_range: Option<[i32; 2]>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorViewResponse {
    pub kind: String, // "file" | "directory"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewed_range: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TextEditorEditArgs {
    pub command: String,
    pub path: String,
    #[serde(default)]
    pub old_str: Option<String>,
    #[serde(default)]
    pub new_str: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TextEditorEditResponse {}

pub async fn text_editor_view(
    _state: &KdeMcp,
    args: TextEditorViewArgs,
) -> Result<TextEditorViewResponse, ToolError> {
    let path = Path::new(&args.path);
    if !path.is_absolute() {
        return Err(ToolError::PermissionDenied {
            reason: format!("absolute path required, got: {}", args.path),
        });
    }
    let meta = tokio::fs::metadata(path).await.map_err(|e| ToolError::Io {
        path: args.path.clone(),
        error: e.to_string(),
    })?;
    if meta.is_dir() {
        return view_directory(path).await;
    }
    view_file(path, args.view_range).await
}

async fn view_file(
    path: &Path,
    range: Option<[i32; 2]>,
) -> Result<TextEditorViewResponse, ToolError> {
    let body = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| ToolError::Io {
            path: path.to_string_lossy().to_string(),
            error: e.to_string(),
        })?;
    // split_inclusive('\n') keeps trailing newline characters on each entry so
    // that re-joining produces an identical file body. An empty file gives a
    // single empty "line" — we special-case that by treating total = 0.
    let lines: Vec<&str> = if body.is_empty() {
        Vec::new()
    } else {
        body.split_inclusive('\n').collect()
    };
    let total = lines.len();
    let (start, end) = resolve_range(range, total)?;
    let mut out = String::with_capacity(body.len() + total * 6);
    if total > 0 {
        for (i, line) in lines
            .iter()
            .enumerate()
            .skip(start - 1)
            .take(end - start + 1)
        {
            let n = i + 1;
            out.push_str(&format!("{n}\t{}", line));
        }
    }
    Ok(TextEditorViewResponse {
        kind: "file".into(),
        content: Some(out),
        total_lines: Some(total),
        viewed_range: Some([start as i32, end as i32]),
        entries: None,
    })
}

fn resolve_range(range: Option<[i32; 2]>, total: usize) -> Result<(usize, usize), ToolError> {
    if total == 0 {
        // Empty file: range is moot. Report [1, 0] so the caller knows nothing
        // was viewed.
        return Ok((1, 0));
    }
    let Some([s, e]) = range else {
        return Ok((1, total));
    };
    if s < 1 {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range start must be >= 1, got {s}"),
        });
    }
    let start = s as usize;
    let end = if e == -1 {
        total
    } else if e < 1 {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range end must be -1 or >= 1, got {e}"),
        });
    } else {
        (e as usize).min(total)
    };
    if start > end {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("view_range start {start} > end {end}"),
        });
    }
    Ok((start, end))
}

async fn view_directory(root: &Path) -> Result<TextEditorViewResponse, ToolError> {
    let mut entries: Vec<String> = Vec::new();
    walk(root, root, 0, 2, &mut entries)
        .await
        .map_err(|e| ToolError::Io {
            path: root.to_string_lossy().to_string(),
            error: e,
        })?;
    entries.sort();
    Ok(TextEditorViewResponse {
        kind: "directory".into(),
        content: None,
        total_lines: None,
        viewed_range: None,
        entries: Some(entries),
    })
}

fn walk<'a>(
    root: &'a Path,
    current: &'a Path,
    depth: u32,
    max_depth: u32,
    out: &'a mut Vec<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let mut rd = tokio::fs::read_dir(current)
            .await
            .map_err(|e| e.to_string())?;
        while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            let p = entry.path();
            let rel = p.strip_prefix(root).unwrap_or(&p);
            out.push(format!("./{}", rel.display()));
            let ft = entry.file_type().await.map_err(|e| e.to_string())?;
            if ft.is_dir() && depth + 1 < max_depth {
                walk(root, &p, depth + 1, max_depth, out).await?;
            }
        }
        Ok(())
    })
}

pub async fn text_editor_edit(
    _state: &KdeMcp,
    _args: TextEditorEditArgs,
) -> Result<TextEditorEditResponse, ToolError> {
    Err(ToolError::NotImplemented)
}
