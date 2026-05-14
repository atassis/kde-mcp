use super::BootstrappedState;
use crate::error::ToolError;

/// Phase 5 lands the real virtual-mode bootstrap:
///   1. dbus-run-session -- sleep infinity      (private session bus)
///   2. kwin_wayland --virtual on a random Wayland socket
///   3. at-spi-bus-launcher --launch-immediately
///   4. wayvnc (only if --vnc=<port> was passed)
///   5. EIS handoff against the private bus
pub async fn bootstrap() -> Result<BootstrappedState, ToolError> {
    Err(ToolError::NotImplemented)
}
