//! Argument parsing and the read -> filter -> aggregate -> render pipeline.

use std::io;
use std::path::{Path, PathBuf};

use crate::aggregate::aggregate;
use crate::parse::parse_jsonl;
use crate::report::{render_report, RenderOptions};

/// Parsed command-line arguments.
pub struct Args {
    /// Lookback window in milliseconds; `None` means "all time".
    pub since_ms: Option<u64>,
    pub audit_path: PathBuf,
}

/// Parse a `--since` value: `7d` / `1h` / `all`. `None` means all time.
pub fn parse_since(s: &str) -> Result<Option<u64>, String> {
    if s == "all" {
        return Ok(None);
    }
    let err = || format!("unrecognised --since value: {s} (use 7d / 1h / all)");
    match s.as_bytes().split_last() {
        Some((&unit, digits))
            if (unit == b'd' || unit == b'h')
                && !digits.is_empty()
                && digits.iter().all(u8::is_ascii_digit) =>
        {
            let n: u64 = std::str::from_utf8(digits)
                .unwrap()
                .parse()
                .map_err(|_| err())?;
            let ms = if unit == b'd' {
                n * 24 * 60 * 60 * 1000
            } else {
                n * 60 * 60 * 1000
            };
            Ok(Some(ms))
        }
        _ => Err(err()),
    }
}

/// Default audit-log location: `~/.local/state/kde-mcp/audit.jsonl`.
pub fn default_audit_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/state/kde-mcp/audit.jsonl")
}

/// Parse `--since <value>` and `--audit <path>` from an argument vector.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut since_ms = None;
    let mut audit_path = default_audit_path();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--since" if i + 1 < argv.len() => {
                since_ms = parse_since(&argv[i + 1])?;
                i += 2;
            }
            "--audit" if i + 1 < argv.len() => {
                audit_path = PathBuf::from(&argv[i + 1]);
                i += 2;
            }
            _ => i += 1,
        }
    }
    Ok(Args {
        since_ms,
        audit_path,
    })
}

/// Human label for a lookback window, e.g. `"7 days"` or `"1 hour"`.
pub fn describe_ms(ms: u64) -> String {
    let day_ms = 24.0 * 60.0 * 60.0 * 1000.0;
    let hour_ms = 60.0 * 60.0 * 1000.0;
    let days = (ms as f64 / day_ms).round() as i64;
    if days >= 1 {
        format!("{days} day{}", if days == 1 { "" } else { "s" })
    } else {
        let hours = (ms as f64 / hour_ms).round() as i64;
        format!("{hours} hour{}", if hours == 1 { "" } else { "s" })
    }
}

/// Read the audit log at `path`, keep records at or after `now_ms - since_ms`
/// (records with an unparseable timestamp are always kept), aggregate, and
/// render the report. The end date comes from the latest valid included
/// timestamp; `fallback_ending_label` is used only when none is valid.
pub fn report_for(
    path: &Path,
    since_ms: Option<u64>,
    now_ms: i64,
    fallback_ending_label: &str,
) -> io::Result<String> {
    let records = parse_jsonl(path)?;
    let kept: Vec<_> = match since_ms {
        None => records,
        Some(ms) => {
            let cutoff = now_ms - ms as i64;
            records
                .into_iter()
                .filter(|r| match chrono::DateTime::parse_from_rfc3339(&r.ts) {
                    Ok(dt) => dt.timestamp_millis() >= cutoff,
                    Err(_) => true, // unparseable timestamp -> keep
                })
                .collect()
        }
    };
    let ending_label = kept
        .iter()
        .filter_map(|record| chrono::DateTime::parse_from_rfc3339(&record.ts).ok())
        .max()
        .map(|ts| {
            ts.with_timezone(&chrono::Utc)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| fallback_ending_label.to_string());
    let stats = aggregate(&kept);
    let since_label = match since_ms {
        None => "all time".to_string(),
        Some(ms) => describe_ms(ms),
    };
    Ok(render_report(
        &stats,
        &RenderOptions {
            since_label,
            ending_label,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    const MIN_MS: i64 = 60 * 1000;
    const TIMINGS: &str = r#""timings":{"t_received_ns":0,"t_policy_done_ns":1000000,"t_impl_start_ns":1100000,"t_impl_done_ns":50000000,"t_audit_written_ns":50500000}"#;

    fn write_temp(rows: &[(&str, i64)]) -> tempfile::NamedTempFile {
        use chrono::{TimeZone, Utc};
        let mut s = String::new();
        for (tool, ts_ms) in rows {
            let ts = Utc.timestamp_millis_opt(*ts_ms).unwrap().to_rfc3339();
            s.push_str(&format!(
                "{{\"tool\":\"{tool}\",\"ts\":\"{ts}\",{TIMINGS}}}\n"
            ));
        }
        use std::io::Write;
        let mut log = tempfile::NamedTempFile::new().unwrap();
        log.write_all(s.as_bytes()).unwrap();
        log.flush().unwrap();
        log
    }

    #[test]
    fn parse_since_accepts_7d_1h_all() {
        assert_eq!(parse_since("7d").unwrap(), Some(7 * 24 * 60 * 60 * 1000));
        assert_eq!(parse_since("1h").unwrap(), Some(60 * 60 * 1000));
        assert_eq!(parse_since("all").unwrap(), None);
    }

    #[test]
    fn parse_since_rejects_malformed_input() {
        assert!(parse_since("seven days").is_err());
    }

    #[test]
    fn parse_args_reads_since_and_audit() {
        let argv = ["--since", "1h", "--audit", "/tmp/x.jsonl"].map(String::from);
        let args = parse_args(&argv).unwrap();
        assert_eq!(args.since_ms, Some(60 * 60 * 1000));
        assert_eq!(args.audit_path, PathBuf::from("/tmp/x.jsonl"));
    }

    #[test]
    fn parse_args_defaults() {
        let args = parse_args(&[]).unwrap();
        assert_eq!(args.since_ms, None);
        assert!(args.audit_path.to_str().unwrap().ends_with("audit.jsonl"));
    }

    #[test]
    fn report_for_since_all_keeps_every_record() {
        let now = 1_700_000_000_000i64;
        let log = write_temp(&[
            ("ancient_tool", now - 30 * DAY_MS),
            ("recent_tool", now - 10 * MIN_MS),
            ("recent_tool", now - 5 * MIN_MS),
        ]);
        let out = report_for(log.path(), None, now, "2026-01-01").unwrap();
        assert!(out.contains("N = 3 tool calls"));
        assert!(out.contains("ancient_tool"));
        assert!(out.contains("recent_tool"));
    }

    #[test]
    fn report_for_since_1h_filters_out_old_records() {
        let now = 1_700_000_000_000i64;
        let log = write_temp(&[
            ("ancient_tool", now - 30 * DAY_MS),
            ("recent_tool", now - 10 * MIN_MS),
            ("recent_tool", now - 5 * MIN_MS),
        ]);
        let out = report_for(log.path(), Some(60 * 60 * 1000), now, "2026-01-01").unwrap();
        assert!(out.contains("N = 2 tool calls"));
        assert!(out.contains("recent_tool"));
        assert!(!out.contains("ancient_tool"));
    }
    #[test]
    fn report_date_uses_utc_and_filters_old_records() {
        let mut log = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(
            log,
            r#"{{"tool":"old","ts":"2020-01-01T00:00:00Z",{TIMINGS}}}"#
        )
        .unwrap();
        writeln!(
            log,
            r#"{{"tool":"recent","ts":"2025-01-01T00:30:00+02:00",{TIMINGS}}}"#
        )
        .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2024-12-31T23:00:00Z")
            .unwrap()
            .timestamp_millis();
        let report = report_for(log.path(), Some(60 * 60 * 1000), now, "2026-01-01").unwrap();
        assert!(report.contains("ending 2024-12-31"));
        assert!(!report.contains("| old"));
    }

    #[test]
    fn empty_or_invalid_timestamp_input_uses_supplied_fallback_date() {
        let mut log = tempfile::NamedTempFile::new().unwrap();
        assert!(report_for(log.path(), None, 0, "2026-01-01")
            .unwrap()
            .contains("ending 2026-01-01"));
        use std::io::Write;
        writeln!(log, r#"{{"tool":"observe","ts":"invalid",{TIMINGS}}}"#).unwrap();
        assert!(report_for(log.path(), None, 0, "2026-01-01")
            .unwrap()
            .contains("ending 2026-01-01"));
    }

    #[test]
    fn excluded_valid_timestamps_do_not_override_fallback_date() {
        let mut log = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(
            log,
            r#"{{"tool":"old","ts":"2020-01-01T00:00:00Z",{TIMINGS}}}"#
        )
        .unwrap();
        writeln!(log, r#"{{"tool":"observe","ts":"invalid",{TIMINGS}}}"#).unwrap();
        let report = report_for(log.path(), Some(1), 1_700_000_000_000, "2026-01-01").unwrap();
        assert!(report.contains("ending 2026-01-01"));
        assert!(report.contains("N = 1 tool calls"));
    }
}
