//! KWin Scripting loadScript runner.
//!
//! Caller-hosted D-Bus receiver pattern (kdotool-style). The QJSEngine has
//! no file I/O; the only way out is `callDBus`. Task 5 implements this.
