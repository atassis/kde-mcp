use crate::policy::{ApprovalState, save_persistent_approvals};
use notify_rust::{ActionResponse, Hint, Notification, NotificationHandle, Urgency};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn notify_lease_acquired(mode: &str) {
    let mode_descriptor = match mode {
        "live" => "your live desktop",
        "virtual" => "a virtual desktop session",
        _ => "your desktop",
    };
    let body = format!("kde-mcp is controlling {mode_descriptor}. Press Meta+Esc to abort.");
    fire_blocking("Claude is using your computer", body, Urgency::Normal, 5000).await;
}

pub async fn notify_lease_released(mode: &str) {
    let body = format!("kde-mcp finished using {mode} mode.");
    fire_blocking(
        "Claude finished using your computer",
        body,
        Urgency::Low,
        3000,
    )
    .await;
}

/// Fire the per-app approval notification (spec §6.0 step 5) and wire its
/// action buttons back into the in-memory approvals map. Returns the
/// notification id for the caller to embed in the `app_approval_required`
/// response.
///
/// Action buttons require a live D-Bus signal listener on the
/// `NotificationHandle`; without one, Plasma 6 still renders the buttons
/// but clicking them does nothing. The listener is spawned on a detached
/// `std::thread` (because notify-rust's `wait_for_action` is blocking and
/// consumes the handle); when the user clicks, it mutates the approvals
/// map (and persists Session approvals to disk) so the next gate check
/// observes the new state.
pub async fn request_app_approval(
    app_class: &str,
    approvals: Arc<Mutex<HashMap<String, ApprovalState>>>,
    store_path: Option<PathBuf>,
) -> Option<u32> {
    let app = app_class.to_string();
    let app_for_show = app.clone();

    // Step 1: synchronously fire the notification on a blocking thread.
    // notify-rust's `.show()` is sync and creates a zbus Connection it owns.
    let handle: NotificationHandle = match tokio::task::spawn_blocking(move || {
        Notification::new()
            .summary(&format!("Allow Claude to control {app_for_show}?"))
            .body("Claude is requesting permission to send input to this app.")
            .appname("kde-mcp")
            .hint(Hint::Urgency(Urgency::Critical))
            .action("approve-session", "Allow this session")
            .action("approve-once", "Allow once")
            .action("deny", "Deny")
            .timeout(notify_rust::Timeout::Never)
            .show()
    })
    .await
    {
        Ok(Ok(h)) => h,
        _ => return None,
    };
    let id = handle.id();

    // Step 2: drive `wait_for_action_async` on a tokio task so the zbus
    // signal listener has a live async runtime to poll. The previous version
    // used `std::thread::spawn(|| handle.wait_for_action(...))` (sync), which
    // silently swallowed action signals on Plasma 6 — D-Bus confirmed via
    // an external listener that `ActionInvoked` was emitted but the sync
    // wrapper's internal `zbus::block_on` did not deliver it. The async path
    // keeps the handle's zbus reactor on a polled context.
    let app_for_listener = app.clone();
    tokio::spawn(async move {
        // Keep `handle` alive for the lifetime of the wait.
        handle
            .wait_for_action_async(move |action_resp| {
                let state = match action_resp {
                    ActionResponse::Custom(s) => match *s {
                        "approve-session" => Some(ApprovalState::Session),
                        "approve-once" => Some(ApprovalState::Once),
                        "deny" => Some(ApprovalState::Denied),
                        _ => None,
                    },
                    ActionResponse::Closed(_) => None,
                };
                if let Some(state) = state {
                    // Closure is sync; mutate via a spawned tokio task so
                    // we can `.await` the tokio Mutex.
                    tokio::spawn(async move {
                        let mut map = approvals.lock().await;
                        map.insert(app_for_listener, state);
                        if matches!(state, ApprovalState::Session)
                            && let Some(p) = &store_path
                        {
                            save_persistent_approvals(p, &map);
                        }
                    });
                }
            })
            .await;
    });

    Some(id)
}

/// Fire the "Claude aborted by user" notification when Meta+Esc is pressed.
pub async fn notify_cancelled() {
    fire_blocking(
        "Claude aborted by user",
        "All in-flight tool calls have been cancelled.".to_string(),
        Urgency::Normal,
        3000,
    )
    .await;
}

async fn fire_blocking(summary: &'static str, body: String, urgency: Urgency, timeout_ms: u32) {
    // notify-rust's .show() is blocking; offload it to a blocking thread so
    // the tokio runtime stays responsive. Errors are intentionally swallowed
    // — a failed notification must not block startup or shutdown. (A user
    // running without org.freedesktop.Notifications is rare on KDE but should
    // not break kde-mcp.)
    let _ = tokio::task::spawn_blocking(move || {
        let _ = Notification::new()
            .summary(summary)
            .body(&body)
            .appname("kde-mcp")
            .hint(Hint::Urgency(urgency))
            .timeout(notify_rust::Timeout::Milliseconds(timeout_ms))
            .show();
    })
    .await;
}

#[cfg(test)]
mod tests {
    // notify-rust ultimately calls org.freedesktop.Notifications on the user's
    // session bus. Unit-testing that surface requires either mocking D-Bus or
    // running an end-to-end smoke check; both are deferred to the manual smoke
    // procedure in tests/integration/SMOKE.md (Task 13). What we can verify
    // here is that the public surface compiles and the function signatures
    // don't bit-rot.
    #[test]
    fn module_compiles_and_exports_two_async_fns() {
        let _ = super::notify_lease_acquired;
        let _ = super::notify_lease_released;
    }
}
