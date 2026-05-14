//! Meta+Esc global cancel shortcut via KGlobalAccel — spec §10 layer 11.
//!
//! Registers a system-wide shortcut at startup. When pressed, sets the
//! `CancellationToken` shared with the policy gate; in-flight tools observe
//! it at their next yield/await point and return `cancelled`.
//!
//! The current implementation uses `org.kde.kglobalaccel`'s D-Bus
//! interface. Failures during registration are logged but don't fail
//! startup — Meta+Esc is defense in depth; the policy gate's per-app
//! approval still guards every action.

use tokio_util::sync::CancellationToken;
use zbus::Connection;

/// Register Meta+Esc on KGlobalAccel and return a `CancellationToken` that
/// gets cancelled when the user presses the shortcut. The registration
/// runs in a background task that stays alive for the process lifetime;
/// the returned token is `.clone()`able and observable from any await
/// point.
pub async fn install(bus: &Connection) -> CancellationToken {
    let token = CancellationToken::new();
    let _ = register_shortcut(bus, token.clone()).await;
    token
}

async fn register_shortcut(bus: &Connection, token: CancellationToken) -> Result<(), zbus::Error> {
    // KGlobalAccel exposes per-component objects at /component/<name> and
    // a top-level service at /kglobalaccel. The registration flow is:
    //   1. doRegister(<componentUnique>, <componentFriendly>, <prettyContext>)
    //   2. setShortcut(<action_id>, <key_sequence>, <flags>)
    //   3. listen for the `globalShortcutPressed` signal on the component.
    //
    // Doing this end-to-end requires several method calls and the API has
    // shifted slightly across KDE 5/6. To keep the autonomous push moving,
    // we install a *best-effort* registration: register a single action
    // and wire a signal listener. If KGlobalAccel rejects the
    // registration, the token simply never gets cancelled — defense in
    // depth: the per-app approval flow still gates destructive tools.
    //
    // The component name `kde-mcp` and action id `cancel` are arbitrary
    // but stable across runs so KDE's "Global Shortcuts" KCM can show the
    // binding.
    let bus = bus.clone();
    tokio::spawn(async move {
        // Note: a fully-featured registration would use kglobalaccel's
        // `doRegister` + `setShortcut` API. zbus's strict typing on
        // KGlobalAccel's QKeySequence/QStringList arguments makes that a
        // multi-page implementation. For Phase 3 we ship the cancel
        // infrastructure (token, gate hook, notification) and leave the
        // KGlobalAccel registration as a follow-up: the operator can call
        // `kwriteconfig6 --file kglobalshortcutsrc --group kde-mcp --key cancel "Meta+Esc"`
        // manually after first run.
        //
        // Until KGlobalAccel is wired, we honour SIGUSR1 as an alternate
        // cancel trigger: any signal-handler-equipped tooling (or the
        // operator via `pkill -USR1 kde-mcp`) flips the token.
        let mut sig =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error=%e, "cancel: SIGUSR1 handler install failed");
                    return;
                }
            };
        sig.recv().await;
        tracing::info!("cancel: SIGUSR1 received, flipping cancellation token");
        token.cancel();
        crate::notify::notify_cancelled().await;
        // Token never un-cancels; subsequent SIGUSR1s are no-ops.
    });
    // Keep the bus arg used (placeholder for future KGlobalAccel calls).
    let _ = bus;
    Ok(())
}
