use notify_rust::{Hint, Notification, Urgency};

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

/// Fire the per-app approval notification (spec §6.0 step 5). Returns the
/// notification ID for the caller to embed in the `app_approval_required`
/// response. The notification's action-button handler is wired separately
/// in `src/cancel.rs` / `src/server.rs` startup — this function only emits.
pub async fn request_app_approval(app_class: &str) -> Option<u32> {
    let app = app_class.to_string();
    tokio::task::spawn_blocking(move || -> Option<u32> {
        let handle = Notification::new()
            .summary(&format!("Allow Claude to control {app}?"))
            .body("Claude is requesting permission to send input to this app.")
            .appname("kde-mcp")
            .hint(Hint::Urgency(Urgency::Critical))
            .action("approve-session", "Allow this session")
            .action("approve-once", "Allow once")
            .action("deny", "Deny")
            .timeout(notify_rust::Timeout::Never)
            .show()
            .ok()?;
        Some(handle.id())
    })
    .await
    .ok()
    .flatten()
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
