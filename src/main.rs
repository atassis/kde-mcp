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

    let server = KdeMcp::new(cli.mode, audit_log, bootstrapped.bus);

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
