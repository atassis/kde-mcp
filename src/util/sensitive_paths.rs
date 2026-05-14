//! Sensitive-path detection for text_editor.edit / bash warnings — spec §10
//! layer 9. Non-blocking; informational only.

use std::path::Path;

const PATTERNS: &[&str] = &[
    ".ssh/",
    ".gnupg/",
    ".config/keepassxc/",
    ".config/git/",
    ".aws/",
    ".kube/",
    "/etc/",
    "/boot/",
    "/root/",
];

/// Returns a warning string if `path` (string form) hits any sensitive
/// pattern. `~` is expanded against `$HOME` so user-provided paths like
/// `~/.ssh/id_rsa` match. Absolute paths under `/etc/`, `/boot/`, `/root/`
/// match directly.
pub fn warning_for_path(path: &str) -> Option<String> {
    let expanded = expand_tilde(path);
    let p = Path::new(&expanded);
    let pstr = p.to_string_lossy();
    for pat in PATTERNS {
        if pstr.contains(pat) {
            return Some(format!("sensitive path: {pat}"));
        }
    }
    None
}

/// Returns warnings for any sensitive-path substring within a shell command.
/// Matches the raw command text (no shell tokenisation) — over-approximate
/// but cheap. Used by `bash`.
pub fn warnings_for_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in PATTERNS {
        if cmd.contains(pat) {
            out.push(format!("sensitive path mentioned: {pat}"));
        }
    }
    out
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return format!("{}/{}", home.to_string_lossy(), rest);
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_path_warns() {
        assert!(warning_for_path("~/.ssh/id_rsa").is_some());
        assert!(warning_for_path("/home/user/.ssh/config").is_some());
    }

    #[test]
    fn etc_path_warns() {
        assert!(warning_for_path("/etc/passwd").is_some());
    }

    #[test]
    fn benign_path_silent() {
        assert!(warning_for_path("/tmp/foo").is_none());
        assert!(warning_for_path("~/Documents/note.md").is_none());
    }

    #[test]
    fn command_substring_warns() {
        let w = warnings_for_command("cat ~/.ssh/id_rsa | head");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains(".ssh"));
    }

    #[test]
    fn command_with_multiple_sensitive_paths() {
        let w = warnings_for_command("cp /etc/hosts ~/.ssh/backup");
        assert_eq!(w.len(), 2);
    }
}
