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
    /// Daemon-mode session attribution (ADR-0012). Omitted in stdio mode,
    /// where there's only one session per process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Redact sensitive args before audit-log persistence per spec §9.
///
/// - `clipboard_write.content`: always hashed (never plaintext).
/// - Everything else passes through verbatim.
pub fn redact(tool: &str, args: Value) -> Value {
    let Value::Object(mut map) = args else {
        return args;
    };
    if tool == "clipboard_write" {
        if let Some(content) = map.get_mut("content") {
            *content = json!({ "sha256": placeholder_hash(content) });
        }
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

/// Write a lifecycle event (daemon_start, daemon_idle_stop, session_open,
/// session_close, etc.) to the audit log. Uses `tool` as the event-type
/// slug for cheap grep / jq filtering. `extra` is merged into the `args`
/// column.
impl AuditLog {
    pub async fn write_event(&self, event: &'static str, extra: Value) -> std::io::Result<()> {
        self.write(AuditRecord {
            ts: String::new(),
            mode: "live",
            tool: event.to_string(),
            args: extra,
            ok: true,
            duration_ms: None,
            error: None,
            vision_fallback: None,
            session_id: None,
        })
        .await
    }
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

/// Rotate the audit log when it reaches this size (spec §9: "rotated at
/// 50 MB; keep 5"). Pre-write check; rotation moves the current file to
/// `.1`, shifting `.1` → `.2`, etc., dropping `.5`.
const ROTATE_BYTES: u64 = 50 * 1024 * 1024;
const ROTATE_KEEP: usize = 5;

pub struct AuditLog {
    path: PathBuf,
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
            path: path.to_path_buf(),
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
        // Pre-write rotation check. Cheap: stat the open fd via metadata.
        if let Ok(meta) = f.metadata().await
            && meta.len() + line.len() as u64 > ROTATE_BYTES
        {
            drop(f);
            self.rotate().await?;
            f = self.file.lock().await;
            // After rotate(), file is replaced; reopen the handle.
            *f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?;
        }
        f.write_all(line.as_bytes()).await?;
        f.flush().await
    }

    async fn rotate(&self) -> std::io::Result<()> {
        // Shift .ROTATE_KEEP → drop; .N → .N+1 for N in (0..KEEP-1).
        for i in (0..ROTATE_KEEP).rev() {
            let from = if i == 0 {
                self.path.clone()
            } else {
                self.path.with_extension(format!("jsonl.{i}"))
            };
            let to = self.path.with_extension(format!("jsonl.{}", i + 1));
            if i + 1 > ROTATE_KEEP {
                let _ = tokio::fs::remove_file(&from).await;
                continue;
            }
            if tokio::fs::metadata(&from).await.is_ok() {
                let _ = tokio::fs::rename(&from, &to).await;
            }
        }
        Ok(())
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
            session_id: None,
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
        assert!(
            v.get("session_id").is_none(),
            "session_id should be omitted when None (stdio mode)"
        );
    }

    #[tokio::test]
    async fn writes_session_id_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::open(&path).await.unwrap();
        log.write(AuditRecord {
            ts: String::new(),
            mode: "live",
            tool: "observe".into(),
            args: json!({}),
            ok: true,
            duration_ms: Some(0),
            error: None,
            vision_fallback: None,
            session_id: Some("11111111-2222-3333-4444-555555555555".into()),
        })
        .await
        .unwrap();
        drop(log);
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        let v: Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["session_id"], "11111111-2222-3333-4444-555555555555");
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
    fn unrelated_tools_pass_through_verbatim() {
        let r = redact(
            "mouse",
            json!({"action": "click", "coordinate": [100, 200]}),
        );
        assert_eq!(r["action"], "click");
        assert_eq!(r["coordinate"], json!([100, 200]));
    }
}
