use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub ts: String,
    pub mode: &'static str,
    pub tool: String,
    pub args: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_fallback: Option<bool>,
}

/// Redact sensitive args before audit-log persistence per spec §9.
///
/// - `clipboard_write.content`: always hashed (never plaintext).
/// - `text_editor_edit.{old_str,new_str,content}`: hashed when > 500 chars.
/// - Everything else passes through verbatim.
pub fn redact(tool: &str, args: Value) -> Value {
    let Value::Object(mut map) = args else {
        return args;
    };
    match tool {
        "clipboard_write" => {
            if let Some(content) = map.get_mut("content") {
                *content = json!({ "sha256": placeholder_hash(content) });
            }
        }
        "text_editor_edit" => {
            for key in ["old_str", "new_str", "content"] {
                if let Some(v) = map.get_mut(key) {
                    if let Some(s) = v.as_str() {
                        if s.len() > 500 {
                            let len = s.len();
                            *v = json!({ "sha256": placeholder_hash(v), "len": len });
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Value::Object(map)
}

// Real SHA-256 of the JSON-serialised value. Phase 6 polish replaces the
// Phase 0 placeholder. Format: lowercase hex string prefixed `sha256:`.
fn placeholder_hash(v: &Value) -> String {
    use sha2::{Digest, Sha256};
    let s = if let Some(text) = v.as_str() {
        text.as_bytes().to_vec()
    } else {
        serde_json::to_vec(v).unwrap_or_default()
    };
    let mut hasher = Sha256::new();
    hasher.update(&s);
    let digest = hasher.finalize();
    format!("sha256:{digest:x}")
}

pub fn default_path() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state).join("kde-mcp/audit.jsonl");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/state/kde-mcp/audit.jsonl");
    }
    PathBuf::from("/tmp/kde-mcp/audit.jsonl")
}

pub struct AuditLog {
    file: Mutex<File>,
}

impl AuditLog {
    pub async fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    pub async fn write(&self, mut rec: AuditRecord) -> std::io::Result<()> {
        if rec.ts.is_empty() {
            rec.ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        }
        let mut line = serde_json::to_string(&rec).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut f = self.file.lock().await;
        f.write_all(line.as_bytes()).await?;
        f.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_a_parseable_jsonl_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::open(&path).await.unwrap();
        log.write(AuditRecord {
            ts: String::new(),
            mode: "live",
            tool: "observe".into(),
            args: json!({}),
            ok: true,
            duration_ms: Some(5),
            error: None,
            vision_fallback: None,
        })
        .await
        .unwrap();
        drop(log);

        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["tool"], "observe");
        assert_eq!(v["mode"], "live");
        assert_eq!(v["ok"], true);
        assert!(v["ts"].as_str().unwrap().contains("T"));
        assert!(
            v.get("error").is_none(),
            "error should be omitted when None"
        );
    }

    #[test]
    fn redacts_clipboard_write_content() {
        let r = redact("clipboard_write", json!({"content": "secret"}));
        let hash = r["content"]["sha256"].as_str().expect("sha256");
        assert!(hash.starts_with("sha256:"));
        // Real SHA-256 produces 64 hex chars.
        assert_eq!(hash.len(), "sha256:".len() + 64);
        assert!(r["content"].get("plaintext").is_none());
        assert!(!r.to_string().contains("secret"));
    }

    #[test]
    fn redacted_hash_is_deterministic_and_real_sha256() {
        let r1 = redact("clipboard_write", json!({"content": "hello"}));
        let r2 = redact("clipboard_write", json!({"content": "hello"}));
        assert_eq!(r1["content"]["sha256"], r2["content"]["sha256"]);
        // SHA-256 of "hello" is 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824.
        let h = r1["content"]["sha256"].as_str().unwrap();
        assert_eq!(
            h,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn long_text_editor_strings_are_redacted() {
        let big = "x".repeat(501);
        let r = redact("text_editor_edit", json!({"old_str": big}));
        assert!(r["old_str"]["sha256"].is_string());
        assert_eq!(r["old_str"]["len"], 501);
    }

    #[test]
    fn short_text_editor_strings_pass_through() {
        let r = redact("text_editor_edit", json!({"old_str": "short"}));
        assert_eq!(r["old_str"], "short");
    }

    #[test]
    fn unrelated_tools_pass_through_verbatim() {
        let r = redact("bash", json!({"command": "rm -rf /"}));
        assert_eq!(r["command"], "rm -rf /");
    }
}
