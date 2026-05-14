//! Lazily-opened AT-SPI accessibility-bus connection.
//!
//! Task 11 implements this; Phase-1 skeleton.

use crate::error::ToolError;

pub struct AtspiConnection {
    // Populated by Task 11.
}

pub async fn connect() -> Result<AtspiConnection, ToolError> {
    Err(ToolError::NotImplemented)
}
