use super::BootstrappedState;
use crate::cli::Mode;
use crate::error::ToolError;
use zbus::Connection;

/// Phase 0 live-mode bootstrap.
///
/// Confirms the user session bus is reachable and reports whether KWin is
/// present. Does NOT yet open the AT-SPI connection or the EIS pipe — those
/// land in Phase 1 / Phase 2 respectively (and pull in the corresponding
/// crate dependencies that Phase 0's Cargo.toml deliberately omits).
pub async fn bootstrap() -> Result<BootstrappedState, ToolError> {
    let bus = Connection::session()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    // Sanity check: is org.kde.KWin on the bus?
    let proxy = zbus::fdo::DBusProxy::new(&bus)
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    let names = proxy
        .list_names()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    let kwin_present = names.iter().any(|n| n.as_str() == "org.kde.KWin");
    if kwin_present {
        tracing::info!("live mode bootstrapped; org.kde.KWin present on session bus");
    } else {
        tracing::warn!(
            "org.kde.KWin not present on session bus — \
             tool invocations that need KWin will fail. \
             Is this a KDE Plasma 6 Wayland session?"
        );
    }

    Ok(BootstrappedState {
        mode: Mode::Live,
        bus,
    })
}
