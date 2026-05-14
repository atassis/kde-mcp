use anyhow::{Context, Result};
use kde_mcp::{KdeMcp, audit, cli::parse_and_validate, lock::ModeLock, mode, notify};
use rmcp::{ServiceExt, transport::stdio};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,kde_mcp=debug")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = match parse_and_validate(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => {
            // clap surfaces --help / --version as "errors" with exit_code 0.
            // Print them and exit cleanly; otherwise propagate as a real error.
            e.exit();
        }
    };

    let mode_name = cli.mode.as_str();

    // Virtual mode's bootstrap reads KDE_MCP_VNC_PORT to pass to wayvnc.
    if let Some(port) = cli.vnc {
        // SAFETY: only set before the runtime spawns worker threads that
        // would read this var; bootstrap reads it on the current task.
        unsafe {
            std::env::set_var("KDE_MCP_VNC_PORT", port.to_string());
        }
    }

    let lock =
        ModeLock::acquire(mode_name).with_context(|| format!("acquiring {mode_name} mode lock"))?;
    tracing::info!(lock = %lock.path().display(), "mode lock acquired");

    notify::notify_lock_acquired(mode_name).await;

    let bootstrapped = mode::bootstrap(cli.mode)
        .await
        .with_context(|| format!("bootstrapping {mode_name} mode"))?;

    let audit_path = audit::default_path();
    let audit_log = Arc::new(
        audit::AuditLog::open(&audit_path)
            .await
            .with_context(|| format!("opening audit log at {}", audit_path.display()))?,
    );
    tracing::info!(audit = %audit_path.display(), "audit log open");

    // Install the Meta+Esc cancel token. KGlobalAccel registration is best-
    // effort; SIGUSR1 always works as fallback (`pkill -USR1 kde-mcp`).
    let cancel = kde_mcp::cancel::install(&bootstrapped.bus).await;
    let server = KdeMcp::new_with_cancel(cli.mode, audit_log, bootstrapped.bus, cancel);
    // System-tray indicator + abort menu (StatusNotifierItem). Runs on its
    // own thread/runtime; failures are warnings only.
    kde_mcp::tray::spawn(server.tray.clone());

    eprintln!("kde-mcp: ready (mode={mode_name})");

    let service = server
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("rmcp serve: {e}"))?;

    service.waiting().await?;

    drop(lock);
    notify::notify_lock_released(mode_name).await;
    tracing::info!("shut down cleanly");
    Ok(())
}
