//! Policy gate — spec §6.0.
//!
//! Every destructive tool routes through `PolicyGate::check()` before any
//! side effect. The gate runs six steps in order:
//!
//! 1. **Lease check** — mutating tools acquire the per-session lease at
//!    dispatch (see `src/lease.rs` / ADR-0011). The gate itself does not
//!    re-check; the acquire happens upstream in `dispatch_audited`.
//! 2. **Cancellation** — checks the shared `CancellationToken`. Set by
//!    Meta+Esc via `src/cancel.rs`.
//! 3. **Target-app resolution** — derives the app class from the tool args.
//!    See [`TargetApp`].
//! 4. **Tier check** — looks up the resolved class in the tier table.
//! 5. **Per-app approval** — in-memory `HashMap<class, ApprovalState>`.
//!    First call against an unapproved app emits a KDE notification with
//!    Allow/Deny action buttons; waits up to 60 seconds and resumes the
//!    original call after approval. Overlapping calls share a notification.
//! 6. **Sentinel warning emission** — when the app's policy entry has
//!    `sentinel = "shell_access" | ...`, attaches a non-blocking warning
//!    to the caller's response (returned by `check()`).
//!
//! `clipboard_write` skips steps 3–5 (it isn't per-app). It still hits
//! step 6 via class-less sentinels.

use crate::error::ToolError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::watch;
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
    /// User denied; treat as denied until they reset.
    Denied,
}

/// The target of a destructive tool call. Determines what step 3 of the
/// gate looks up. `Classless` means "this tool is not per-app"
/// (clipboard_write today; ADR-0013 removed the other Classless tools).
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
    approvals: Arc<Mutex<ApprovalBook>>,
    notifier: ApprovalNotifier,
    approval_timeout: Duration,
    cancel: CancellationToken,
    /// Disk-backed Session-approval store. `None` for tests — keeps
    /// approvals in-memory only so parallel tests don't contaminate
    /// each other (or the user's real `~/.local/state/kde-mcp/`).
    store_path: Option<PathBuf>,
}

type ApprovalFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<ApprovalDecision, String>> + Send>>;
type ApprovalNotifier = Arc<dyn Fn(String, CancellationToken) -> ApprovalFuture + Send + Sync>;

#[derive(Default)]
struct ApprovalBook {
    grants: HashMap<String, ApprovalState>,
    pending: HashMap<String, Weak<PendingApproval>>,
}

struct PendingApproval {
    result: watch::Sender<Option<Result<ApprovalDecision, String>>>,
    once_taken: AtomicBool,
    cancel: CancellationToken,
}

impl Drop for PendingApproval {
    fn drop(&mut self) {
        // A disconnected/timed-out last caller must not leave a live prompt
        // whose eventual answer can authorize a different request.
        self.cancel.cancel();
    }
}

/// Persistent approval store: only `Session` decisions get written; `Once`
/// is by design ephemeral, and `Denied` we don't persist because the user
/// can withdraw a one-off deny by just re-approving next time. Path:
/// `~/.local/state/kde-mcp/approvals.json`.
fn approvals_path() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state).join("kde-mcp/approvals.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/state/kde-mcp/approvals.json");
    }
    PathBuf::from("/tmp/kde-mcp-approvals.json")
}

fn load_persistent_approvals(path: &Path) -> HashMap<String, ApprovalState> {
    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(_) => return HashMap::new(),
    };
    // File schema: { "approvals": ["class1", "class2", ...] }
    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut out = HashMap::new();
    if let Some(arr) = v.get("approvals").and_then(|a| a.as_array()) {
        for entry in arr.iter().filter_map(|x| x.as_str()) {
            out.insert(entry.to_string(), ApprovalState::Session);
        }
    }
    out
}

pub(crate) fn save_persistent_approvals(path: &Path, approvals: &HashMap<String, ApprovalState>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Only persist Session approvals; Once is ephemeral and Denied is
    // best left to re-prompt on next process start.
    let persisted: Vec<&str> = approvals
        .iter()
        .filter(|(_, s)| matches!(s, ApprovalState::Session))
        .map(|(k, _)| k.as_str())
        .collect();
    let body = serde_json::json!({"approvals": persisted});
    let _ = std::fs::write(path, body.to_string());
}

impl PolicyGate {
    pub fn new(config: PolicyConfig, cancel: CancellationToken) -> Self {
        let path = approvals_path();
        let approvals = load_persistent_approvals(&path);
        Self {
            config,
            approvals: Arc::new(Mutex::new(ApprovalBook {
                grants: approvals,
                ..Default::default()
            })),
            notifier: Arc::new(|app, cancel| {
                Box::pin(crate::notify::request_app_approval(app, cancel))
            }),
            approval_timeout: Duration::from_secs(60),
            cancel,
            store_path: Some(path),
        }
    }

    /// In-memory gate with no disk persistence — for tests.
    #[cfg(test)]
    pub(crate) fn new_ephemeral(config: PolicyConfig, cancel: CancellationToken) -> Self {
        Self {
            config,
            approvals: Arc::new(Mutex::new(ApprovalBook::default())),
            notifier: Arc::new(|_, cancel| {
                Box::pin(async move {
                    cancel.cancelled().await;
                    Err("cancelled".into())
                })
            }),
            approval_timeout: Duration::from_secs(60),
            cancel,
            store_path: None,
        }
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Run the gate. Returns either `Ok(warnings)` to proceed (warnings
    /// merge into the tool's response) or `Err(ToolError)` to short-circuit.
    pub async fn check(&self, target: TargetApp, action: Action) -> Result<Vec<String>, ToolError> {
        // Step 1: lease acquisition happens upstream in `dispatch_audited`; the
        // gate does not re-check.

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

        // Step 5: the deadline includes delivery and any additional prompt
        // needed when another concurrent call consumes an Allow-once grant.
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => return Err(ToolError::Cancelled {
                cancelled_at_stage: "app_approval".into(),
            }),
            outcome = tokio::time::timeout(self.approval_timeout, self.await_approval(&class)) => {
                outcome.map_err(|_| ToolError::Timeout {
                    closest_match: Some(format!("approval for {class} was not received within 60 seconds")),
                    current_windows: vec![],
                })??;
            }
        }
        if self.cancel.is_cancelled() {
            return Err(ToolError::Cancelled {
                cancelled_at_stage: "app_approval".into(),
            });
        }

        // Step 6: sentinel warning.
        let mut warnings = Vec::new();
        if let Some(s) = policy.sentinel {
            warnings.push(s.message(&class));
        }
        Ok(warnings)
    }

    async fn await_approval(&self, class: &str) -> Result<(), ToolError> {
        loop {
            let pending = {
                let mut book = self.approvals.lock();
                match book.grants.get(class).copied() {
                    Some(ApprovalState::Session) => return Ok(()),
                    Some(ApprovalState::Once) => {
                        book.grants.remove(class);
                        return Ok(());
                    }
                    Some(ApprovalState::Denied) => {
                        book.grants.remove(class);
                        return Err(ToolError::PermissionDenied {
                            reason: format!("user denied {class}"),
                        });
                    }
                    None => {}
                }
                if let Some(pending) = book.pending.get(class).and_then(Weak::upgrade) {
                    pending
                } else {
                    let (result, _) = watch::channel(None);
                    let pending = Arc::new(PendingApproval {
                        result,
                        once_taken: AtomicBool::new(false),
                        cancel: self.cancel.child_token(),
                    });
                    book.pending.insert(class.into(), Arc::downgrade(&pending));
                    let notifier = self.notifier.clone();
                    let app = class.to_string();
                    let cancel = pending.cancel.clone();
                    let result = pending.result.clone();
                    tokio::spawn(async move {
                        // The notifier owns notification cleanup. It must observe
                        // cancellation even when the last tool future is dropped.
                        let decision = notifier(app, cancel).await;
                        result.send_replace(Some(decision));
                    });
                    pending
                }
            };
            let mut result = pending.result.subscribe();
            let decision = result
                .wait_for(|value| value.is_some())
                .await
                .map_err(|_| ToolError::PermissionDenied {
                    reason: "approval listener stopped".into(),
                })?
                .clone();
            let decision = decision
                .ok_or_else(|| ToolError::PermissionDenied {
                    reason: "approval listener returned no decision".into(),
                })?
                .map_err(|reason| ToolError::PermissionDenied {
                    reason: format!("could not obtain approval for {class}: {reason}"),
                })?;
            match decision {
                ApprovalDecision::Session => {
                    let mut book = self.approvals.lock();
                    book.grants.insert(class.into(), ApprovalState::Session);
                    if let Some(path) = &self.store_path {
                        save_persistent_approvals(path, &book.grants);
                    }
                    return Ok(());
                }
                ApprovalDecision::Once => {
                    let taken = pending.once_taken.swap(true, Ordering::SeqCst);
                    let mut book = self.approvals.lock();
                    if book
                        .pending
                        .get(class)
                        .is_some_and(|p| p.ptr_eq(&Arc::downgrade(&pending)))
                    {
                        book.pending.remove(class);
                    }
                    if !taken {
                        return Ok(());
                    }
                }
                ApprovalDecision::Deny | ApprovalDecision::Dismissed => {
                    return Err(ToolError::PermissionDenied {
                        reason: format!("user denied or dismissed approval for {class}"),
                    });
                }
            }
        }
    }

    /// Supply a decision without contacting the desktop in unit tests.
    #[cfg(test)]
    pub async fn record_approval(&self, app_class: &str, decision: ApprovalDecision) {
        let mut book = self.approvals.lock();
        if let Some(pending) = book.pending.get(app_class).and_then(Weak::upgrade) {
            pending.result.send_replace(Some(Ok(decision)));
        } else {
            book.grants.insert(
                app_class.into(),
                match decision {
                    ApprovalDecision::Session => ApprovalState::Session,
                    ApprovalDecision::Once => ApprovalState::Once,
                    ApprovalDecision::Deny | ApprovalDecision::Dismissed => ApprovalState::Denied,
                },
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ApprovalDecision {
    Once,
    Session,
    Deny,
    Dismissed,
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
        PolicyGate::new_ephemeral(PolicyConfig::default(), CancellationToken::new())
    }

    #[tokio::test]
    async fn original_call_waits_for_approval() {
        let g = empty_gate();
        let call = g.check(TargetApp::Class("kate".into()), Action::Generic);
        tokio::pin!(call);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut call)
                .await
                .is_err(),
            "unanswered approval must keep the original call pending"
        );
        g.record_approval("kate", ApprovalDecision::Session).await;
        assert!(call.await.is_ok());
    }

    type TestRequest = (
        String,
        CancellationToken,
        tokio::sync::oneshot::Sender<Result<ApprovalDecision, String>>,
    );

    fn controlled_gate(
        timeout: Duration,
    ) -> (
        Arc<PolicyGate>,
        tokio::sync::mpsc::UnboundedReceiver<TestRequest>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut gate = empty_gate();
        gate.approval_timeout = timeout;
        gate.notifier = Arc::new(move |app, cancel| {
            let (answer, response) = tokio::sync::oneshot::channel();
            tx.send((app, cancel.clone(), answer)).unwrap();
            Box::pin(async move {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err("cancelled".into()),
                    result = response => result.unwrap_or_else(|_| Err("listener stopped".into())),
                }
            })
        });
        (Arc::new(gate), rx)
    }

    fn start_check(g: Arc<PolicyGate>) -> tokio::task::JoinHandle<Result<Vec<String>, ToolError>> {
        tokio::spawn(async move {
            g.check(TargetApp::Class("kate".into()), Action::Generic)
                .await
        })
    }

    #[tokio::test]
    async fn concurrent_calls_share_session_prompt() {
        let (g, mut requests) = controlled_gate(Duration::from_secs(1));
        let first = start_check(g.clone());
        let (_, _, answer) = requests.recv().await.unwrap();
        let second = start_check(g.clone());
        tokio::task::yield_now().await;
        assert!(requests.try_recv().is_err());
        assert!(!first.is_finished());
        assert!(!second.is_finished());
        answer.send(Ok(ApprovalDecision::Session)).unwrap();
        assert!(first.await.unwrap().is_ok());
        assert!(second.await.unwrap().is_ok());
        assert!(
            g.check(TargetApp::Class("kate".into()), Action::Generic)
                .await
                .is_ok()
        );
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn once_allows_exactly_one_concurrent_call() {
        let (g, mut requests) = controlled_gate(Duration::from_secs(1));
        let first = start_check(g.clone());
        let (_, _, answer) = requests.recv().await.unwrap();
        let second = start_check(g.clone());
        tokio::task::yield_now().await;
        answer.send(Ok(ApprovalDecision::Once)).unwrap();
        let (_, _, next_answer) = requests.recv().await.unwrap();
        tokio::task::yield_now().await;
        assert_ne!(first.is_finished(), second.is_finished());
        next_answer.send(Ok(ApprovalDecision::Deny)).unwrap();
        let outcomes = [first.await.unwrap(), second.await.unwrap()];
        assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| matches!(o, Err(ToolError::PermissionDenied { .. })))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn deny_dismissal_and_delivery_failure_fail_closed() {
        for answer_value in [
            Ok(ApprovalDecision::Deny),
            Ok(ApprovalDecision::Dismissed),
            Err("bus unavailable".into()),
        ] {
            let (g, mut requests) = controlled_gate(Duration::from_secs(1));
            let call = start_check(g);
            let (_, _, answer) = requests.recv().await.unwrap();
            answer.send(answer_value).unwrap();
            assert!(matches!(
                call.await.unwrap(),
                Err(ToolError::PermissionDenied { .. })
            ));
        }
    }

    #[tokio::test]
    async fn timeout_cancels_prompt_and_late_answer_cannot_grant() {
        let (g, mut requests) = controlled_gate(Duration::from_millis(30));
        let call = start_check(g.clone());
        let (_, cancel, late_answer) = requests.recv().await.unwrap();
        assert!(matches!(
            call.await.unwrap(),
            Err(ToolError::Timeout { .. })
        ));
        assert!(cancel.is_cancelled());
        let _ = late_answer.send(Ok(ApprovalDecision::Session));
        let next_call = start_check(g);
        let (_, _, answer) = requests.recv().await.unwrap();
        assert!(!next_call.is_finished());
        answer.send(Ok(ApprovalDecision::Deny)).unwrap();
        assert!(next_call.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn cancellation_stops_waiting_and_closes_prompt() {
        let (g, mut requests) = controlled_gate(Duration::from_secs(1));
        let call = start_check(g.clone());
        let (_, prompt_cancel, _) = requests.recv().await.unwrap();
        g.cancel.cancel();
        assert!(matches!(
            call.await.unwrap(),
            Err(ToolError::Cancelled { .. })
        ));
        assert!(prompt_cancel.is_cancelled());
    }

    #[tokio::test]
    async fn dropping_last_waiter_cancels_prompt_but_not_other_waiters() {
        let (g, mut requests) = controlled_gate(Duration::from_secs(1));
        let first = start_check(g.clone());
        let (_, cancel, answer) = requests.recv().await.unwrap();
        let second = start_check(g);
        tokio::task::yield_now().await;
        first.abort();
        let _ = first.await;
        assert!(!cancel.is_cancelled());
        second.abort();
        let _ = second.await;
        assert!(cancel.is_cancelled());
        let _ = answer.send(Ok(ApprovalDecision::Session));
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
        let g = PolicyGate::new_ephemeral(config, CancellationToken::new());
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
        let g = PolicyGate::new_ephemeral(config, CancellationToken::new());
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
        let g = PolicyGate::new_ephemeral(config, CancellationToken::new());
        // Need to approve first.
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
        let g = PolicyGate::new_ephemeral(config, CancellationToken::new());
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
        let g = PolicyGate::new_ephemeral(config, CancellationToken::new());
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
        let g = PolicyGate::new_ephemeral(PolicyConfig::default(), token.clone());
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
