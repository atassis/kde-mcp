use crate::error::ToolError;
use crate::server::KdeMcp;
use crate::trust::{Tagged, Trust};
use crate::util::sensitive_paths;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BashArgs {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub restart: Option<bool>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BashResponse {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: Tagged<String>,
    pub stderr: Tagged<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_output: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn bash(state: &KdeMcp, args: BashArgs) -> Result<BashResponse, ToolError> {
    if args.restart.unwrap_or(false) {
        let mut guard = state.bash_session.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
        return Ok(BashResponse {
            ok: true,
            exit_code: None,
            stdout: Tagged {
                trust: Trust::Trusted,
                text: "".into(),
            },
            stderr: Tagged {
                trust: Trust::Trusted,
                text: "session restarted".into(),
            },
            truncated: false,
            full_output: None,
            warnings: vec![],
        });
    }

    let command = args.command.ok_or_else(|| ToolError::UnsupportedRegion {
        reason: "command is required (unless restart=true)".into(),
    })?;

    // Blocklist (spec §10 layer 8). Cannot be disabled via tool args.
    check_blocklist(&command)?;

    let mut warnings = sensitive_paths::warnings_for_command(&command);
    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));

    // Lazy PTY spawn. The persistent session means `cd /tmp; pwd` works
    // across calls until restart=true.
    let session_arc = ensure_session(state)?;
    let outcome =
        tokio::task::spawn_blocking(move || run_command_in_session(session_arc, &command, timeout))
            .await
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("bash join: {e}"),
            })??;

    let (stdout, truncated_out) = truncate(outcome.stdout);
    let (stderr, _) = truncate(outcome.stderr);
    if truncated_out {
        warnings.push("stdout truncated at 16KB — full_output not yet emitted".into());
    }

    Ok(BashResponse {
        ok: outcome.exit_code == Some(0),
        exit_code: outcome.exit_code,
        stdout: Tagged {
            trust: Trust::External,
            text: stdout,
        },
        stderr: Tagged {
            trust: Trust::External,
            text: stderr,
        },
        truncated: truncated_out,
        full_output: None,
        warnings,
    })
}

fn truncate(s: String) -> (String, bool) {
    if s.len() <= MAX_OUTPUT_BYTES {
        return (s, false);
    }
    // Truncate at a UTF-8 boundary near MAX_OUTPUT_BYTES.
    let mut cut = MAX_OUTPUT_BYTES;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let truncated = format!("{}\n[…truncated at {MAX_OUTPUT_BYTES} bytes…]", &s[..cut]);
    (truncated, true)
}

pub struct BashSession {
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

fn ensure_session(state: &KdeMcp) -> Result<std::sync::Arc<Mutex<Option<BashSession>>>, ToolError> {
    let arc = state.bash_session.clone();
    {
        let mut guard = arc.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(spawn_session()?);
        }
    }
    Ok(arc)
}

fn spawn_session() -> Result<BashSession, ToolError> {
    let pty = NativePtySystem::default();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("openpty: {e}"),
        })?;
    let mut cmd = CommandBuilder::new("bash");
    cmd.arg("--noprofile");
    cmd.arg("--norc");
    // Inherit env from the calling process.
    for (k, v) in std::env::vars_os() {
        cmd.env(k, v);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("spawn bash: {e}"),
        })?;
    drop(pair.slave);
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("pty take_writer: {e}"),
        })?;
    Ok(BashSession {
        master: pair.master,
        _child: child,
        writer,
    })
}

struct CommandOutcome {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

fn run_command_in_session(
    session_arc: std::sync::Arc<Mutex<Option<BashSession>>>,
    command: &str,
    timeout: Duration,
) -> Result<CommandOutcome, ToolError> {
    let mut guard = session_arc.lock().unwrap_or_else(|e| e.into_inner());
    let session = guard
        .as_mut()
        .ok_or_else(|| ToolError::ExternalCommandFailed {
            stderr: "bash session unexpectedly absent".into(),
        })?;

    // Sentinel marker so we know when the command's output ends.
    let marker = format!("__KDEMCP_END_{}__", uuid_simple());
    // Run the user's command, then `echo <marker>:$?` so we can read the
    // exit code from the same stream.
    let script = format!("{command}\nprintf '%s\\n' \"{marker}:$?\"\n");
    session
        .writer
        .write_all(script.as_bytes())
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("pty write: {e}"),
        })?;
    session
        .writer
        .flush()
        .map_err(|e| ToolError::ExternalCommandFailed {
            stderr: format!("pty flush: {e}"),
        })?;

    let mut reader =
        session
            .master
            .try_clone_reader()
            .map_err(|e| ToolError::ExternalCommandFailed {
                stderr: format!("pty reader: {e}"),
            })?;

    let mut buf = String::new();
    let mut chunk = [0u8; 4096];
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let s = String::from_utf8_lossy(&chunk[..n]);
                buf.push_str(&s);
                if buf.contains(&marker) {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return Err(ToolError::ExternalCommandFailed {
                    stderr: format!("pty read: {e}"),
                });
            }
        }
    }
    if !buf.contains(&marker) {
        return Err(ToolError::Timeout {
            closest_match: None,
            current_windows: vec![],
        });
    }

    // Strip the echo of the command itself (PTY tends to echo input back)
    // and the trailing marker line. Also strip any prompt fragments before
    // returning. The marker line is `<marker>:<exit>`.
    let raw = strip_ansi_escapes::strip_str(&buf);
    let (stdout, exit_code) = split_at_marker(&raw, &marker);
    Ok(CommandOutcome {
        stdout,
        stderr: String::new(),
        exit_code,
    })
}

fn split_at_marker(raw: &str, marker: &str) -> (String, Option<i32>) {
    if let Some(idx) = raw.find(marker) {
        let before = &raw[..idx];
        let after = &raw[idx + marker.len()..];
        let exit = after
            .trim_start_matches(':')
            .lines()
            .next()
            .and_then(|s| s.trim().parse::<i32>().ok());
        let cleaned = strip_prompt_lines(before);
        return (cleaned, exit);
    }
    (raw.to_string(), None)
}

fn strip_prompt_lines(s: &str) -> String {
    // Drop bash's prompt echo of the command itself. Heuristic: the input
    // line shows up before the actual output; strip the first line if it
    // ends with the command itself (PTY echo behaviour). For simplicity
    // and robustness, we just trim leading/trailing whitespace; full prompt
    // stripping is a Phase 6 polish item.
    s.trim().to_string()
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn check_blocklist(cmd: &str) -> Result<(), ToolError> {
    let trimmed = cmd.trim();
    let lower = trimmed.to_ascii_lowercase();
    // Patterns are deliberately blunt — these are oh-shit prevention, not a
    // security boundary.
    let blocked: &[(&str, &str)] = &[
        ("rm -rf /", "destroys the filesystem"),
        ("rm -rf /*", "destroys the filesystem"),
        ("rm -rf ~", "destroys the home directory"),
        ("rm -rf $home", "destroys the home directory"),
        (":(){:|:&};:", "fork bomb"),
        (":(){ :|:& };:", "fork bomb"),
    ];
    for (pat, why) in blocked {
        if lower.contains(pat) {
            return Err(ToolError::PermissionDenied {
                reason: format!("blocklist:{pat} — {why}"),
            });
        }
    }
    // dd of=/dev/sd*
    if lower.contains("dd ") && lower.contains("of=/dev/sd") {
        return Err(ToolError::PermissionDenied {
            reason: "blocklist:dd of=/dev/sd* — writes to block device".into(),
        });
    }
    // mkfs.* against block devices
    if lower.starts_with("mkfs.") || lower.contains(" mkfs.") {
        return Err(ToolError::PermissionDenied {
            reason: "blocklist:mkfs.* — formats a block device".into(),
        });
    }
    // sudo / pkexec (unless server was started with --allow-sudo; this
    // bash tool doesn't currently read that flag — Phase 6 polish wires it).
    if lower.starts_with("sudo ") || lower.starts_with("pkexec ") {
        return Err(ToolError::PermissionDenied {
            reason: "blocklist:sudo/pkexec — privilege escalation".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocklist_rejects_rm_rf_root() {
        let err = check_blocklist("rm -rf /").unwrap_err();
        assert_eq!(err.code(), "permission_denied");
    }

    #[test]
    fn blocklist_rejects_rm_rf_home() {
        assert!(check_blocklist("rm -rf ~").is_err());
        assert!(check_blocklist("rm -rf $HOME").is_err());
    }

    #[test]
    fn blocklist_rejects_dd_block_device() {
        assert!(check_blocklist("dd if=/dev/zero of=/dev/sda").is_err());
        assert!(check_blocklist("dd of=/dev/sdb1 bs=1M").is_err());
    }

    #[test]
    fn blocklist_rejects_mkfs() {
        assert!(check_blocklist("mkfs.ext4 /dev/sda1").is_err());
    }

    #[test]
    fn blocklist_rejects_fork_bomb() {
        assert!(check_blocklist(":(){:|:&};:").is_err());
    }

    #[test]
    fn blocklist_rejects_sudo_and_pkexec() {
        assert!(check_blocklist("sudo ls").is_err());
        assert!(check_blocklist("pkexec systemctl restart sshd").is_err());
    }

    #[test]
    fn benign_commands_pass() {
        assert!(check_blocklist("ls -la").is_ok());
        assert!(check_blocklist("cat /etc/hostname").is_ok());
        assert!(check_blocklist("echo hello | wc -l").is_ok());
    }

    #[test]
    fn truncate_under_limit_unchanged() {
        let s = "a".repeat(1000);
        let (out, t) = truncate(s.clone());
        assert_eq!(out, s);
        assert!(!t);
    }

    #[test]
    fn truncate_at_limit_marks_truncated() {
        let s = "a".repeat(MAX_OUTPUT_BYTES + 100);
        let (out, t) = truncate(s);
        assert!(t);
        assert!(out.contains("truncated"));
    }
}
