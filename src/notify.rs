use crate::policy::ApprovalDecision;
use notify_rust::{ActionResponse, Hint, Notification, NotificationHandle, Urgency};
use tokio_util::sync::CancellationToken;

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

/// Own the notification until it is answered or its last waiting call ends.
/// Decisions are returned to the gate; this task never grants permission.
pub async fn request_app_approval(
    app_class: String,
    cancel: CancellationToken,
) -> Result<ApprovalDecision, String> {
    // Complete delivery even if cancellation arrives in flight: once the
    // server returns its id we can close it rather than orphaning a prompt.
    let handle: NotificationHandle = Notification::new()
        .summary(&format!("Allow Claude to control {app_class}?"))
        .body("Claude is requesting permission to send input to this app.")
        .appname("kde-mcp")
        .hint(Hint::Urgency(Urgency::Critical))
        .action("approve-session", "Allow this session")
        .action("approve-once", "Allow once")
        .action("deny", "Deny")
        .timeout(notify_rust::Timeout::Never)
        .show_async()
        .await
        .map_err(|error| format!("notification delivery failed: {error}"))?;
    let mut decision = ApprovalDecision::Dismissed;
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {},
        _ = handle.wait_for_action_async(|response| {
            decision = match response {
                ActionResponse::Custom("approve-session") => ApprovalDecision::Session,
                ActionResponse::Custom("approve-once") => ApprovalDecision::Once,
                ActionResponse::Custom("deny") => ApprovalDecision::Deny,
                _ => ApprovalDecision::Dismissed,
            };
        }) => {},
    }
    // Bound cleanup as well; a broken notification service must not hold
    // an approved call indefinitely. Closing is best-effort on bus loss.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle.close_async()).await;
    if cancel.is_cancelled() {
        return Err("approval request cancelled".into());
    }
    Ok(decision)
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
    // The approval lifecycle is exercised against a mock notification
    // service on a private bus in tests/integration/approval_notification.rs.
    #[test]
    fn module_compiles_and_exports_two_async_fns() {
        let _ = super::notify_lease_acquired;
        let _ = super::notify_lease_released;
    }
}
