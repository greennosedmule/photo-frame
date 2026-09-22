//! One full scan: promote, reconcile, generate derivatives, record status.

use crate::ctx::Ctx;
use crate::scan::Scanner;
use crate::{derivatives, promote};

pub async fn scan_cycle(ctx: &Ctx, scanner: &mut Scanner) -> anyhow::Result<()> {
    let result = run(ctx, scanner).await;
    // Whatever happened, the panel should not claim we are still indexing.
    ctx.store.set_meta("indexing_state", "idle").await?;
    if result.is_ok() {
        ctx.store
            .set_meta("last_scan_at", &photoframe_store::now())
            .await?;
    }
    result
}

async fn run(ctx: &Ctx, scanner: &mut Scanner) -> anyhow::Result<()> {
    ctx.store.set_meta("indexing_state", "scanning").await?;

    if !ctx.config.ingest_require_scan {
        let p = promote::promote(ctx).await?;
        if p != Default::default() {
            tracing::info!(?p, "incoming processed");
        }
    }
    let s = scanner.reconcile(ctx).await?;
    if s.added + s.moved + s.removed > 0 {
        tracing::info!(?s, "library reconciled");
    }
    if ctx.stopping() {
        return Ok(());
    }

    let queued = ctx.store.status().await?.derivative_queue;
    if queued > 0 {
        ctx.store.set_meta("indexing_state", "generating").await?;
        ctx.store
            .set_meta("scan_total", &queued.to_string())
            .await?;
        ctx.store.set_meta("scan_done", "0").await?;
        let d = derivatives::run(ctx).await?;
        tracing::info!(?d, "derivatives processed");
        ctx.store.set_meta("scan_done", &queued.to_string()).await?;
    }
    Ok(())
}
