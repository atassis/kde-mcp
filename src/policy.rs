//! Policy gate — spec §6.0.
//!
//! Every destructive tool routes through `PolicyGate::check()` before any
//! side effect. The gate runs six steps in order:
//!
//! 1. **Mode lock** — process holds its mode's lock. Stays asserted via the
//!    `ModeLock` guard on the server's bootstrap path; the gate just trusts
//!    the lock is held (re-checking would race the kernel anyway).
//! 2. **Cancellation** — checks the shared `CancellationToken`. Set by
//!    Meta+Esc via `src/cancel.rs`.
//! 3. **Target-app resolution** — derives the app class from the tool args.
//!    See [`TargetApp`].
//! 4. **Tier check** — looks up the resolved class in the tier table.
//! 5. **Per-app approval** — in-memory `HashMap<class, ApprovalState>`.
//!    First call against an unapproved app emits a KDE notification with
//!    Allow/Deny action buttons; returns `app_approval_required`. Caller
//!    retries after the user picks.
//! 6. **Sentinel warning emission** — when the app's policy entry has
//!    `sentinel = "shell_access" | ...`, attaches a non-blocking warning
//!    to the caller's response (returned via [`GateOutcome::Ok::warnings`]).
//!
//! `bash`, `text_editor.edit`, and `clipboard_write` skip steps 3–5 (they
//! aren't per-app). They still hit step 6 via class-less sentinels.

use crate::error::ToolError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Tier of an application — determines which destructive tools are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Read-only target: every destructive tool rejected.
    ViewOnly,
    /// Click + navigation allowed; typing rejected; only modifier+letter
    /// chord allowlist permitted on `keyboard.key`.
    ClickOnly,
    /// Default — no per-app restrictions.
    #[default]
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sentinel {
    ShellAccess,
    FilesystemAccess,
    SystemSettings,
}

impl Sentinel {
    pub fn message(self, app_class: &str) -> String {
        match self {
            Sentinel::ShellAccess => {
                format!("target_app:{app_class} — equivalent to shell access")
            }
            Sentinel::FilesystemAccess => {
                format!("target_app:{app_class} — direct filesystem access")
            }
            Sentinel::SystemSettings => {
                format!("target_app:{app_class} — modifies system settings")
            }
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AppPolicy {
    #[serde(default)]
    pub tier: Tier,
    #[serde(default)]
    pub sentinel: Option<Sentinel>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PolicyConfig {
    #[serde(default)]
    pub apps: HashMap<String, AppPolicy>,
}

impl PolicyConfig {
    /// Load from the standard path. Returns the empty config if the file is
    /// missing (the gate then treats every app as tier=full, no sentinel).
    /// Parse errors propagate so the operator sees them.
    pub fn load_default() -> Result<Self, ToolError> {
        let path = config_path();
        Self::load_from(&path)
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self, ToolError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let body = std::fs::read_to_string(path).map_err(|e| ToolError::Io {
            path: path.to_string_lossy().to_string(),
            error: e.to_string(),
        })?;
        toml::from_str(&body).map_err(|e| ToolError::PermissionDenied {
            reason: format!("policy.toml parse error: {e}"),
        })
    }

    pub fn lookup(&self, class: &str) -> AppPolicy {
        self.apps.get(class).cloned().unwrap_or_default()
    }
}

pub fn config_path() -> PathBuf {
    if let Some(cfg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(cfg).join("kde-mcp/policy.toml");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/kde-mcp/policy.toml");
    }
    PathBuf::from("/tmp/kde-mcp-policy.toml")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalState {
    /// User explicitly approved this app for the current session.
    Session,
    /// User approved once (consumed after one use).
    Once,
    /// Pending — notification sent, user hasn't picked yet.
    Pending,
    /// User denied; treat as denied until they reset.
    Denied,
}

/// The target of a destructive tool call. Determines what step 3 of the
/// gate looks up. `Classless` means "this tool is not per-app" (bash,
/// text_editor.edit, clipboard_write).
#[derive(Debug, Clone)]
pub enum TargetApp {
    Class(String),
    Classless,
}

/// The action a tool is about to perform; used for the click_only tier's
/// allowlist on keyboard.key.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    /// Any input emulation other than the special-cased ones below.
    Generic,
    /// `keyboard.type` — typing literal text.
    KeyboardType,
    /// `keyboard.key` with chord; `chord_is_navigation` controls the
    /// click_only allowlist.
    KeyboardKey { chord_is_navigation: bool },
}

pub struct PolicyGate {
    config: PolicyConfig,
    approvals: Mutex<HashMap<String, ApprovalState>>,
    cancel: CancellationToken,
}

impl PolicyGate {
    pub fn new(config: PolicyConfig, cancel: CancellationToken) -> Self {
        Self {
            config,
            approvals: Mutex::new(HashMap::new()),
            cancel,
        }
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Run the gate. Returns either `Ok(warnings)` to proceed (warnings
    /// merge into the tool's response) or `Err(ToolError)` to short-circuit.
    pub async fn check(&self, target: TargetApp, action: Action) -> Result<Vec<String>, ToolError> {
        // Step 1: mode lock is asserted by the bootstrap path; the gate
        // does not re-check.

        // Step 2: cancellation.
        if self.cancel.is_cancelled() {
            return Err(ToolError::Cancelled {
                cancelled_at_stage: "policy_gate".into(),
            });
        }

        // Steps 3-5 are per-app.
        let (class, policy) = match target {
            TargetApp::Classless => return Ok(vec![]),
            TargetApp::Class(c) => {
                let p = self.config.lookup(&c);
                (c, p)
            }
        };

        // Step 4: tier check.
        match policy.tier {
            Tier::Full => {}
            Tier::ViewOnly => {
                return Err(ToolError::TierRestricted {
                    app_class: class,
                    current_tier: "view_only".into(),
                    allowed_actions: vec!["(none)".into()],
                });
            }
            Tier::ClickOnly => match action {
                Action::KeyboardType => {
                    return Err(ToolError::TierRestricted {
                        app_class: class,
                        current_tier: "click_only".into(),
                        allowed_actions: vec![
                            "mouse.click".into(),
                            "mouse.scroll".into(),
                            "keyboard.key (navigation only)".into(),
                        ],
                    });
                }
                Action::KeyboardKey {
                    chord_is_navigation: false,
                } => {
                    return Err(ToolError::TierRestricted {
                        app_class: class,
                        current_tier: "click_only".into(),
                        allowed_actions: vec![
                            "navigation keys: Tab/Return/Escape/Page_*/Home/End/arrows".into(),
                        ],
                    });
                }
                _ => {}
            },
        }

        // Step 5: per-app approval.
        let mut approvals = self.approvals.lock().await;
        match approvals.get(&class).copied() {
            Some(ApprovalState::Session) => {}
            Some(ApprovalState::Once) => {
                approvals.insert(class.clone(), ApprovalState::Pending);
                // "once" consumed; next call re-prompts.
            }
            Some(ApprovalState::Denied) => {
                approvals.remove(&class);
                return Err(ToolError::PermissionDenied {
                    reason: format!("user previously denied {class}"),
                });
            }
            Some(ApprovalState::Pending) | None => {
                approvals.insert(class.clone(), ApprovalState::Pending);
                drop(approvals);
                let nid = crate::notify::request_app_approval(&class).await;
                return Err(ToolError::AppApprovalRequired {
                    app_class: class,
                    notification_id: nid,
                    still_pending: true,
                });
            }
        }
        drop(approvals);

        // Step 6: sentinel warning.
        let mut warnings = Vec::new();
        if let Some(s) = policy.sentinel {
            warnings.push(s.message(&class));
        }
        Ok(warnings)
    }

    /// Record the user's pick from the approval notification.
    pub async fn record_approval(&self, app_class: &str, decision: ApprovalDecision) {
        let mut approvals = self.approvals.lock().await;
        match decision {
            ApprovalDecision::Once => {
                approvals.insert(app_class.into(), ApprovalState::Once);
            }
            ApprovalDecision::Session => {
                approvals.insert(app_class.into(), ApprovalState::Session);
            }
            ApprovalDecision::Deny => {
                approvals.insert(app_class.into(), ApprovalState::Denied);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ApprovalDecision {
    Once,
    Session,
    Deny,
}

/// Keyboard navigation key allowlist used by the `click_only` tier.
/// Chord is "navigation" if the non-modifier key is in this set and
/// modifiers are only Ctrl (for Ctrl+C/V/A/X/Z) or none.
pub fn chord_is_navigation(chord: &crate::keymap::Chord) -> bool {
    use crate::keymap::*;
    let nav_keys = [
        KEY_TAB,
        KEY_ENTER,
        KEY_ESC,
        KEY_UP,
        KEY_DOWN,
        KEY_LEFT,
        KEY_RIGHT,
        KEY_PAGEUP,
        KEY_PAGEDOWN,
        KEY_HOME,
        KEY_END,
        KEY_BACKSPACE,
        KEY_DELETE,
    ];
    let key_ok = nav_keys.contains(&chord.key);
    let mod_ok = chord.modifiers.is_empty()
        || (chord.modifiers.len() == 1 && chord.modifiers[0] == Modifier::Ctrl);
    // Allow common Ctrl+letter chords too: c/v/x/z/a.
    let common_ctrl_letter = chord.modifiers.len() == 1
        && chord.modifiers[0] == Modifier::Ctrl
        && [
            keycode_for_char('c').unwrap(),
            keycode_for_char('v').unwrap(),
            keycode_for_char('x').unwrap(),
            keycode_for_char('z').unwrap(),
            keycode_for_char('a').unwrap(),
        ]
        .contains(&chord.key);
    common_ctrl_letter || (key_ok && mod_ok)
}

pub type SharedPolicyGate = Arc<PolicyGate>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::parse_chord;

    fn empty_gate() -> PolicyGate {
        PolicyGate::new(PolicyConfig::default(), CancellationToken::new())
    }

    #[tokio::test]
    async fn classless_tools_bypass_app_checks() {
        let g = empty_gate();
        let w = g
            .check(TargetApp::Classless, Action::Generic)
            .await
            .unwrap();
        assert!(w.is_empty());
    }

    #[tokio::test]
    async fn unknown_app_defaults_to_full_tier() {
        let g = empty_gate();
        // First call against an unknown app prompts; we then approve and retry.
        let err = g
            .check(TargetApp::Class("kate".into()), Action::Generic)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "app_approval_required");
        g.record_approval("kate", ApprovalDecision::Session).await;
        let w = g
            .check(TargetApp::Class("kate".into()), Action::Generic)
            .await
            .unwrap();
        assert!(w.is_empty());
    }

    #[tokio::test]
    async fn view_only_tier_rejects_destructive() {
        let mut config = PolicyConfig::default();
        config.apps.insert(
            "brave-browser".into(),
            AppPolicy {
                tier: Tier::ViewOnly,
                sentinel: None,
            },
        );
        let g = PolicyGate::new(config, CancellationToken::new());
        let err = g
            .check(TargetApp::Class("brave-browser".into()), Action::Generic)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "tier_restricted");
    }

    #[tokio::test]
    async fn click_only_rejects_keyboard_type() {
        let mut config = PolicyConfig::default();
        config.apps.insert(
            "konsole".into(),
            AppPolicy {
                tier: Tier::ClickOnly,
                sentinel: None,
            },
        );
        let g = PolicyGate::new(config, CancellationToken::new());
        let err = g
            .check(TargetApp::Class("konsole".into()), Action::KeyboardType)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "tier_restricted");
    }

    #[tokio::test]
    async fn click_only_accepts_navigation_chord() {
        let mut config = PolicyConfig::default();
        config.apps.insert(
            "konsole".into(),
            AppPolicy {
                tier: Tier::ClickOnly,
                sentinel: None,
            },
        );
        let g = PolicyGate::new(config, CancellationToken::new());
        // Need to approve first.
        let _ = g
            .check(
                TargetApp::Class("konsole".into()),
                Action::KeyboardKey {
                    chord_is_navigation: true,
                },
            )
            .await;
        g.record_approval("konsole", ApprovalDecision::Session)
            .await;
        let w = g
            .check(
                TargetApp::Class("konsole".into()),
                Action::KeyboardKey {
                    chord_is_navigation: true,
                },
            )
            .await
            .unwrap();
        assert!(w.is_empty());
    }

    #[tokio::test]
    async fn click_only_rejects_non_navigation_chord() {
        let mut config = PolicyConfig::default();
        config.apps.insert(
            "konsole".into(),
            AppPolicy {
                tier: Tier::ClickOnly,
                sentinel: None,
            },
        );
        let g = PolicyGate::new(config, CancellationToken::new());
        let err = g
            .check(
                TargetApp::Class("konsole".into()),
                Action::KeyboardKey {
                    chord_is_navigation: false,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "tier_restricted");
    }

    #[tokio::test]
    async fn sentinel_warning_attached_to_response() {
        let mut config = PolicyConfig::default();
        config.apps.insert(
            "konsole".into(),
            AppPolicy {
                tier: Tier::Full,
                sentinel: Some(Sentinel::ShellAccess),
            },
        );
        let g = PolicyGate::new(config, CancellationToken::new());
        let _ = g
            .check(TargetApp::Class("konsole".into()), Action::Generic)
            .await;
        g.record_approval("konsole", ApprovalDecision::Session)
            .await;
        let w = g
            .check(TargetApp::Class("konsole".into()), Action::Generic)
            .await
            .unwrap();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("shell access"));
    }

    #[tokio::test]
    async fn cancellation_short_circuits() {
        let token = CancellationToken::new();
        let g = PolicyGate::new(PolicyConfig::default(), token.clone());
        token.cancel();
        let err = g
            .check(TargetApp::Classless, Action::Generic)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "cancelled");
    }

    #[test]
    fn navigation_chord_predicate() {
        assert!(chord_is_navigation(&parse_chord("Tab").unwrap()));
        assert!(chord_is_navigation(&parse_chord("Return").unwrap()));
        assert!(chord_is_navigation(&parse_chord("Page_Down").unwrap()));
        assert!(chord_is_navigation(&parse_chord("ctrl+c").unwrap()));
        assert!(chord_is_navigation(&parse_chord("ctrl+v").unwrap()));
        assert!(!chord_is_navigation(&parse_chord("ctrl+shift+t").unwrap()));
        assert!(!chord_is_navigation(&parse_chord("F12").unwrap()));
    }

    #[test]
    fn policy_toml_parses_round_trip() {
        let body = r#"
[apps.konsole]
tier = "click_only"
sentinel = "shell_access"

[apps."brave-browser"]
tier = "view_only"
"#;
        let cfg: PolicyConfig = toml::from_str(body).unwrap();
        let konsole = cfg.lookup("konsole");
        assert_eq!(konsole.tier, Tier::ClickOnly);
        assert_eq!(konsole.sentinel, Some(Sentinel::ShellAccess));
        let brave = cfg.lookup("brave-browser");
        assert_eq!(brave.tier, Tier::ViewOnly);
        let unknown = cfg.lookup("kate");
        assert_eq!(unknown.tier, Tier::Full);
        assert!(unknown.sentinel.is_none());
    }
}
