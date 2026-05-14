use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum Mode {
    Live,
    Virtual,
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
