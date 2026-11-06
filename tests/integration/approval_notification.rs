//! Exercise notify-rust on a private session bus, never the user's desktop.
use kde_mcp::policy::ApprovalDecision;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zbus::zvariant::OwnedValue;

#[test]
fn notification_lifecycle_on_private_bus() {
    let state = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("dbus-run-session")
        .arg("--")
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "integration::approval_notification::private_bus_child",
            "--nocapture",
        ])
        .env("KDE_MCP_APPROVAL_TEST_CHILD", "1")
        .env("XDG_STATE_HOME", state.path())
        .status()
        .expect("dbus-run-session is required for the isolated notification test");
    assert!(status.success());
}

struct Notifications {
    events: mpsc::UnboundedSender<&'static str>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        _app_name: String,
        _replaces_id: u32,
        _app_icon: String,
        _summary: String,
        _body: String,
        actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        assert!(actions.contains(&"approve-session".to_string()));
        assert!(actions.contains(&"approve-once".to_string()));
        assert!(actions.contains(&"deny".to_string()));
        self.events.send("shown").unwrap();
        42
    }

    async fn close_notification(&self, id: u32) {
        assert_eq!(id, 42);
        self.events.send("closed").unwrap();
    }
}

#[tokio::test]
#[ignore = "launched by notification_lifecycle_on_private_bus in a private bus"]
async fn private_bus_child() {
    assert_eq!(
        std::env::var("KDE_MCP_APPROVAL_TEST_CHILD").as_deref(),
        Ok("1")
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        let (events, mut rx) = mpsc::unbounded_channel();
        let bus = zbus::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", Notifications { events })
            .unwrap()
            .build()
            .await
            .unwrap();
        for action in [
            "approve-session",
            "approve-once",
            "deny",
            "dismiss",
            "cancel",
        ] {
            let cancel = CancellationToken::new();
            let task_cancel = cancel.clone();
            let call = tokio::spawn(async move {
                kde_mcp::notify::request_app_approval("test.kate".into(), task_cancel).await
            });
            assert_eq!(rx.recv().await, Some("shown"));
            assert!(!call.is_finished());
            if action == "cancel" {
                cancel.cancel();
            } else {
                // notify-rust installs its listener after Notify returns.
                // Repeat the simulated click until subscribed, without relying
                // on a wall-clock guess for D-Bus match-rule registration.
                while !call.is_finished() {
                    if action == "dismiss" {
                        bus.emit_signal(
                            None::<&str>,
                            "/org/freedesktop/Notifications",
                            "org.freedesktop.Notifications",
                            "NotificationClosed",
                            &(42u32, 2u32),
                        )
                        .await
                        .unwrap();
                    } else {
                        bus.emit_signal(
                            None::<&str>,
                            "/org/freedesktop/Notifications",
                            "org.freedesktop.Notifications",
                            "ActionInvoked",
                            &(42u32, action),
                        )
                        .await
                        .unwrap();
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            let result = call.await.unwrap();
            match action {
                "approve-session" => assert!(matches!(result, Ok(ApprovalDecision::Session))),
                "approve-once" => assert!(matches!(result, Ok(ApprovalDecision::Once))),
                "deny" => assert!(matches!(result, Ok(ApprovalDecision::Deny))),
                "dismiss" => assert!(matches!(result, Ok(ApprovalDecision::Dismissed))),
                "cancel" => assert!(result.is_err()),
                _ => unreachable!(),
            }
            assert_eq!(rx.recv().await, Some("closed"));
        }
        // End-to-end gate + native notification decision. The marker models
        // the handler's side-effect boundary after policy.check().
        let gate = kde_mcp::policy::PolicyGate::new(Default::default(), CancellationToken::new());
        let executed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = executed.clone();
        let call = tokio::spawn(async move {
            gate.check(
                kde_mcp::policy::TargetApp::Class("test.kate".into()),
                kde_mcp::policy::Action::Generic,
            )
            .await?;
            marker.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, kde_mcp::error::ToolError>(())
        });
        assert_eq!(rx.recv().await, Some("shown"));
        assert!(!executed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!call.is_finished());
        while !call.is_finished() {
            bus.emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "ActionInvoked",
                &(42u32, "approve-once"),
            )
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(call.await.unwrap().is_ok());
        assert!(executed.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(rx.recv().await, Some("closed"));
    })
    .await
    .expect("private notification lifecycle stalled");
}
