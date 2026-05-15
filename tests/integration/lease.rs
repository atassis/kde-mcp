//! Integration tests for the multi-session lease (ADR-0011).
//!
//! Each test sets `XDG_RUNTIME_DIR` to a fresh tempdir so concurrent test
//! runs don't share lease files.

use chrono::{TimeZone, Utc};
use kde_mcp::lease::{LeaseConfig, LeaseRecord};
use std::time::Duration;

#[test]
fn default_config_matches_adr_0011() {
    let c = LeaseConfig::default();
    assert_eq!(c.idle_timeout, Duration::from_secs(10));
    assert_eq!(c.acquire_timeout, Duration::from_secs(60));
}

#[test]
fn record_round_trips_through_json() {
    let r = LeaseRecord {
        holder_pid: 4242,
        holder_session_id: "01J5N0VYE5J9F1V3Q9SHV4Z9P8".into(),
        holder_intent: "keyboard.type".into(),
        acquired_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 0).unwrap(),
        last_refresh_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 7).unwrap(),
        expires_at: Utc.with_ymd_and_hms(2026, 5, 15, 13, 0, 17).unwrap(),
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(
        s.contains("\"holder_pid\""),
        "on-disk key must be holder_pid per ADR-0011: {s}"
    );
    assert!(
        s.contains("\"last_refresh_at\""),
        "on-disk key must be last_refresh_at per ADR-0011: {s}"
    );
    let parsed: LeaseRecord = serde_json::from_str(&s).unwrap();
    assert_eq!(parsed, r);
}
