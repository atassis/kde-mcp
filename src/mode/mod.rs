pub mod live;
pub mod r#virtual;

use crate::cli::Mode;
use crate::error::ToolError;
use zbus::Connection;

/// What bootstrap returns: a session bus connection and the mode we're in.
/// More state (atspi connection, reis context, kwin proxies) is added in
/// later phases via composition, not by enlarging this struct.
pub struct BootstrappedState {
    pub mode: Mode,
    pub bus: Connection,
}

pub async fn bootstrap(mode: Mode) -> Result<BootstrappedState, ToolError> {
    match mode {
        Mode::Live => live::bootstrap().await,
        Mode::Virtual => r#virtual::bootstrap().await,
    }
}
