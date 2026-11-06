use std::io::Write;
use std::process::Command;

fn run_cli(rows: &[(&str, &str)], since: &str) -> String {
    let mut log = tempfile::NamedTempFile::new().unwrap();
    for (tool, ts) in rows {
        writeln!(log, r#"{{"tool":"{tool}","ts":"{ts}","timings":{{"t_received_ns":0,"t_policy_done_ns":1000000,"t_impl_start_ns":1100000,"t_impl_done_ns":50000000,"t_audit_written_ns":50500000}}}}"#).unwrap();
    }
    log.flush().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kde-mcp-latency-analyzer"))
        .args(["--since", since, "--audit"])
        .arg(log.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn ending_date_comes_from_latest_valid_input_timestamp() {
    let rows = [
        ("observe", "2024-12-25T23:30:00Z"),
        ("mouse", "2024-12-24T23:30:00Z"),
        ("screenshot", "not-a-timestamp"),
    ];
    let report = run_cli(&rows, "all");
    assert!(report.contains("all time ending 2024-12-25"), "{report}");
    assert_eq!(report, run_cli(&rows, "all"));
    assert!(report.contains("N = 3 tool calls"));
}

#[test]
fn since_all_keeps_old_records_in_cli() {
    let report = run_cli(&[("ancient_tool", "2020-01-01T00:00:00Z")], "all");
    assert!(report.contains("ancient_tool"));
    assert!(report.contains("N = 1 tool calls"));
}

#[test]
fn since_one_hour_filters_old_records_in_cli() {
    let recent = chrono::Utc::now().to_rfc3339();
    let report = run_cli(
        &[
            ("ancient_tool", "2020-01-01T00:00:00Z"),
            ("recent_tool", &recent),
        ],
        "1h",
    );
    assert!(!report.contains("ancient_tool"));
    assert!(report.contains("recent_tool"));
    assert!(report.contains("N = 1 tool calls"));
}
