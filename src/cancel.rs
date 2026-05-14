//! Global cancel shortcut — Meta+Esc via KGlobalAccel, with SIGUSR1 fallback.
//!
//! The shortcut is registered with `org.kde.kglobalaccel` so KWin grabs the
//! key at compositor level (prompt-injection-safe — apps never see it).
//! When the user presses Meta+Esc, kglobalaccel fires
//! `globalShortcutPressed` on `/component/kde-mcp`; we listen for that and
//! flip the `CancellationToken` shared with the policy gate.
//!
//! Registration is best-effort. If kglobalaccel rejects (already-registered
//! action, daemon not running, etc.), SIGUSR1 still works as fallback:
//! `pkill -USR1 kde-mcp`. The tray icon's "Abort" menu item is the
//! third route.

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use zbus::Connection;

// Qt key + modifier constants. Combined Qt::Key value for "Meta+Escape".
const QT_KEY_ESCAPE: i32 = 0x01000000;
const QT_MOD_META: i32 = 0x10000000;
const META_ESCAPE: i32 = QT_KEY_ESCAPE | QT_MOD_META;

const COMPONENT_UNIQUE: &str = "kde-mcp";
const COMPONENT_FRIENDLY: &str = "kde-mcp (Claude desktop control)";
const ACTION_UNIQUE: &str = "cancel";
const ACTION_FRIENDLY: &str = "Abort current operation";

#[zbus::proxy(
    interface = "org.kde.KGlobalAccel",
    default_service = "org.kde.kglobalaccel",
    default_path = "/kglobalaccel"
)]
trait KGlobalAccel {
    #[zbus(name = "doRegister")]
    fn do_register(&self, action_id: Vec<&str>) -> zbus::Result<()>;

    #[zbus(name = "setShortcut")]
    fn set_shortcut(
        &self,
        action_id: Vec<&str>,
        keys: Vec<i32>,
        flags: u32,
    ) -> zbus::Result<Vec<i32>>;
}

#[zbus::proxy(
    interface = "org.kde.kglobalaccel.Component",
    default_service = "org.kde.kglobalaccel"
)]
trait Component {
    #[zbus(signal, name = "globalShortcutPressed")]
    fn global_shortcut_pressed(
        &self,
        component_unique: String,
        shortcut_unique: String,
        timestamp: i64,
    ) -> zbus::Result<()>;
}

/// Install the cancel token. Spawns:
///  - SIGUSR1 handler (always works)
///  - KGlobalAccel Meta+Esc registration + signal listener (best-effort)
pub async fn install(bus: &Connection) -> CancellationToken {
    let token = CancellationToken::new();
    spawn_sigusr1(token.clone());
    if let Err(e) = register_kglobalaccel(bus, token.clone()).await {
        tracing::warn!(error=%e, "kglobalaccel registration failed — Meta+Esc inactive; use SIGUSR1 or tray Abort instead");
    } else {
        tracing::info!("Meta+Esc registered with KGlobalAccel");
    }
    token
}

fn spawn_sigusr1(token: CancellationToken) {
    tokio::spawn(async move {
        let mut sig =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error=%e, "SIGUSR1 handler install failed");
                    return;
                }
            };
        sig.recv().await;
        tracing::info!("cancel: SIGUSR1 received");
        token.cancel();
        crate::notify::notify_cancelled().await;
    });
}

async fn register_kglobalaccel(
    bus: &Connection,
    token: CancellationToken,
) -> Result<(), zbus::Error> {
    let kga = KGlobalAccelProxy::new(bus).await?;
    let action_id = vec![
        COMPONENT_UNIQUE,
        COMPONENT_FRIENDLY,
        ACTION_UNIQUE,
        ACTION_FRIENDLY,
    ];
    // doRegister is idempotent for an already-registered component — it
    // resurrects the action. setShortcut then (re)binds it.
    kga.do_register(action_id.clone()).await?;
    // flags=0x4 = NoAutoloading (don't read existing config); 0x2 = SetPresent
    // (mark as the current shortcut). KGlobalAccelD::SetShortcutFlag enum in
    // KDE source. Using NoAutoloading|SetPresent so we set Meta+Esc even if
    // user previously rebound something.
    let _granted = kga.set_shortcut(action_id, vec![META_ESCAPE], 0x6).await?;

    // Listen for the press signal on /component/kde-mcp.
    let component_path = format!("/component/{COMPONENT_UNIQUE}");
    let component = ComponentProxy::builder(bus)
        .path(component_path)?
        .build()
        .await?;
    let mut stream = component.receive_global_shortcut_pressed().await?;
    tokio::spawn(async move {
        if let Some(_msg) = stream.next().await {
            tracing::info!("cancel: Meta+Esc pressed via KGlobalAccel");
            token.cancel();
            crate::notify::notify_cancelled().await;
        }
        // Token never un-cancels; once flipped, subsequent presses are no-
        // ops. The server stays "shut down for input" until restart —
        // intentional: a cancelled session implies the operator wants to
        // stop, full stop.
    });
    Ok(())
}
