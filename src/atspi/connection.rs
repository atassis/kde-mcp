//! Lazily-opened AT-SPI accessibility-bus connection.
//!
//! Phase-1 surface: `AtspiConnection::default()` is cheap and never opens the
//! bus; Task 11 will fill in a `get()` accessor that opens on first use.

#[derive(Default)]
pub struct AtspiConnection {
    // Populated by Task 11 (one-cell holding atspi::AccessibilityConnection).
}
