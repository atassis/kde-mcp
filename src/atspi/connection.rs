//! Lazily-opened AT-SPI accessibility-bus connection.
//!
//! `AtspiConnection::default()` constructs the wrapper without touching the
//! bus. The first `get()` opens the connection; subsequent calls return the
//! cached handle. On bus failure the call returns a `BusDisconnected` error
//! and the cell stays empty so retries get a fresh attempt.

use crate::error::ToolError;
use atspi::AccessibilityConnection;
use tokio::sync::OnceCell;

#[derive(Default)]
pub struct AtspiConnection {
    inner: OnceCell<AccessibilityConnection>,
}

impl AtspiConnection {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self) -> Result<&AccessibilityConnection, ToolError> {
        self.inner
            .get_or_try_init(|| async {
                AccessibilityConnection::new()
                    .await
                    .map_err(|e| ToolError::BusDisconnected {
                        detail: format!("atspi connect: {e}"),
                    })
            })
            .await
    }
}
