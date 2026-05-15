use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum Mode {
    Live,
    Virtual,
}

/// Wire transport for the MCP layer (ADR-0012). `Stdio` is the original
/// one-client-per-process design where Claude Code spawns a kde-mcp child
/// and speaks over stdin/stdout. `Daemon` is the singleton HTTP server on
/// `127.0.0.1:7575`, activated by systemd-user; one process serves any
/// number of concurrent sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "lowercase")]
pub enum Transport {
    /// MCP over stdin/stdout. Default. One client per process; the
    /// process exits on stdin EOF.
    #[default]
    Stdio,
    /// MCP over HTTP on `127.0.0.1:7575`. Long-running; serves any
    /// number of concurrent connections.
    Daemon,
}

#[derive(Debug, Parser)]
#[command(
    name = "kde-mcp",
    version,
    about = "MCP server for KDE Plasma 6 / Wayland desktops"
)]
pub struct Cli {
    /// Operating mode. Live drives the user's real session; Virtual runs an
    /// isolated KWin child session (Phase 5).
    #[arg(long, value_enum)]
    pub mode: Mode,

    /// VNC port for observing the virtual session via wayvnc. Only valid
    /// with --mode=virtual.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1024..))]
    pub vnc: Option<u16>,

    /// Wire transport. `stdio` (default) is one-client-per-process over
    /// stdin/stdout; `daemon` is a long-running HTTP server on
    /// `127.0.0.1:7575` activated by systemd-user. See ADR-0012.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    pub transport: Transport,
}

impl Cli {
    /// Cross-argument validation that clap's derive layer doesn't express
    /// for us. Currently: --vnc requires --mode=virtual.
    pub fn validate(&self) -> Result<(), clap::Error> {
        if self.mode == Mode::Live && self.vnc.is_some() {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--vnc requires --mode=virtual\n",
            ));
        }
        Ok(())
    }
}

/// Parse CLI args and run cross-argument validation. Used by both main.rs
/// and the integration tests.
pub fn parse_and_validate<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    cli.validate()?;
    Ok(cli)
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Live => "live",
            Mode::Virtual => "virtual",
        }
    }
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Daemon => "daemon",
        }
    }
}
