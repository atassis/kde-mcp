use clap::Parser;
use kde_mcp::cli::{Cli, Mode, Transport, parse_and_validate};

#[test]
fn parses_live_mode() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=live"]).unwrap();
    assert_eq!(cli.mode, Mode::Live);
    assert!(cli.vnc.is_none());
}

#[test]
fn parses_virtual_mode_with_vnc() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=virtual", "--vnc=5910"]).unwrap();
    assert_eq!(cli.mode, Mode::Virtual);
    assert_eq!(cli.vnc, Some(5910));
}

#[test]
fn rejects_unknown_mode() {
    let err = Cli::try_parse_from(["kde-mcp", "--mode=invalid"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid"), "actual: {msg}");
}

#[test]
fn rejects_vnc_in_live_mode() {
    let err = parse_and_validate(["kde-mcp", "--mode=live", "--vnc=5910"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.to_lowercase().contains("vnc"), "actual: {msg}");
}

#[test]
fn mode_is_required() {
    let err = Cli::try_parse_from(["kde-mcp"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("mode"), "actual: {msg}");
}

#[test]
fn vnc_port_below_1024_rejected() {
    let err = Cli::try_parse_from(["kde-mcp", "--mode=virtual", "--vnc=22"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("22") || msg.contains("range"), "actual: {msg}");
}

#[test]
fn mode_as_str_matches_lowercase_variant_names() {
    assert_eq!(Mode::Live.as_str(), "live");
    assert_eq!(Mode::Virtual.as_str(), "virtual");
}

#[test]
fn transport_defaults_to_stdio_when_omitted() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=live"]).unwrap();
    assert_eq!(cli.transport, Transport::Stdio);
}

#[test]
fn transport_daemon_accepted() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=live", "--transport=daemon"]).unwrap();
    assert_eq!(cli.transport, Transport::Daemon);
}

#[test]
fn transport_stdio_explicit_works() {
    let cli = Cli::try_parse_from(["kde-mcp", "--mode=live", "--transport=stdio"]).unwrap();
    assert_eq!(cli.transport, Transport::Stdio);
}

#[test]
fn transport_rejects_unknown() {
    let err = Cli::try_parse_from(["kde-mcp", "--mode=live", "--transport=quux"]).unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn transport_as_str_matches_lowercase_variant_names() {
    assert_eq!(Transport::Stdio.as_str(), "stdio");
    assert_eq!(Transport::Daemon.as_str(), "daemon");
}
