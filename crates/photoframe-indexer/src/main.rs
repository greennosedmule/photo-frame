mod config;
mod ctx;
mod cycle;
mod derivatives;
mod fsutil;
mod promote;
mod requests;
mod scan;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use photoframe_store::Error as StoreError;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::Instant;
use tracing_subscriber::EnvFilter;

use crate::ctx::Ctx;

#[derive(Parser)]
#[command(
    version,
    about = "Photo frame indexer: scan, derivatives, promotion, curation export/import"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the polling scanner (default). Exactly one instance may run.
    Run,
    /// Write a curation export (tags, favourites, date overrides).
    Export {
        #[arg(long)]
        out: PathBuf,
    },
    /// Merge a curation export into the database, keyed by content hash.
    Import {
        #[arg(long = "in")]
        input: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config =
        config::Config::from_env().map_err(|e| anyhow::anyhow!("invalid configuration: {e}"))?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&config.log_level).context("invalid LOG_LEVEL")?)
        .init();

    let store = photoframe_store::connect(&config.database_url).await?;
    store.migrate().await?;

    let ctx = Ctx::new(store.clone(), config);

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            // A second instance must exit loudly rather than race on derivatives.
            let lock = match store.acquire_indexer_lock().await {
                Ok(l) => l,
                Err(StoreError::LockHeld) => {
                    bail!("another photoframe-indexer already holds the singleton lock; exiting")
                }
                Err(e) => return Err(e.into()),
            };
            tracing::info!(interval = ?ctx.config.scan_interval, "photoframe-indexer started");
            let result = run(&ctx).await;
            tracing::info!("shutting down");
            store.release_indexer_lock(lock).await?;
            result?;
        }
        Command::Export { out } => {
            requests::cli_export(&ctx, &out).await?;
            println!("wrote {}", out.display());
        }
        Command::Import { input } => {
            let stats = requests::cli_import(&ctx, &input).await?;
            println!(
                "applied {} photographs, skipped {} not in the library",
                stats.applied, stats.skipped_missing
            );
        }
    }
    Ok(())
}

/// How often the request queue and the scan-request flag are checked. Scans
/// themselves run every `SCAN_INTERVAL_SECS`.
const TICK: Duration = Duration::from_secs(2);

async fn run(ctx: &Ctx) -> anyhow::Result<()> {
    for dir in [
        ctx.layout.library(),
        ctx.layout.derivatives(),
        ctx.layout.quarantine(),
        ctx.layout.exports(),
    ] {
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    if !ctx.config.ingest_require_scan {
        tokio::fs::create_dir_all(ctx.layout.incoming()).await?;
    }
    // A crash mid-request leaves it `running`; run it again.
    ctx.store.reset_running_requests().await?;

    let mut term = signal(SignalKind::terminate())?;
    let mut scanner = scan::Scanner::default();
    let mut next_scan = Instant::now(); // cold start scans immediately
    loop {
        if ctx.store.take_scan_request().await? {
            tracing::info!("manual scan requested");
            next_scan = Instant::now();
        }
        match requests::process(ctx).await {
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "request processing failed"),
        }
        if Instant::now() >= next_scan {
            if let Err(e) = cycle::scan_cycle(ctx, &mut scanner).await {
                tracing::error!(error = %format!("{e:#}"), "scan failed; will retry next interval");
            }
            next_scan = Instant::now() + ctx.config.scan_interval;
        }
        if ctx.stopping() {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = term.recv() => { ctx.stop(); break; }
            _ = tokio::signal::ctrl_c() => { ctx.stop(); break; }
        }
    }
    Ok(())
}
