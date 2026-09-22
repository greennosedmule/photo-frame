//! Derivative jobs: a bounded queue of idempotent, hash-keyed work.

use std::sync::Arc;

use imagepipe::VARIANTS;
use photoframe_store::PhotoRef;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::ctx::Ctx;
use crate::fsutil;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DerivativeStats {
    pub generated: usize,
    pub already_present: usize,
    pub failed: usize,
}

const BATCH: i64 = 32;

/// Process everything eligible, then return. Failed photographs back off
/// exponentially and are picked up again by a later scan.
pub async fn run(ctx: &Ctx) -> anyhow::Result<DerivativeStats> {
    let mut stats = DerivativeStats::default();
    let permits = Arc::new(Semaphore::new(ctx.config.derivative_workers));
    loop {
        if ctx.stopping() {
            break;
        }
        let batch = ctx
            .store
            .pending_derivatives(&photoframe_store::now(), BATCH)
            .await?;
        if batch.is_empty() {
            break;
        }
        let mut set = JoinSet::new();
        for photo in batch {
            let permit = permits.clone().acquire_owned().await?;
            let ctx = ctx.clone();
            set.spawn(async move {
                let _permit = permit;
                process(&ctx, &photo).await
            });
        }
        while let Some(res) = set.join_next().await {
            match res? {
                Outcome::Generated => stats.generated += 1,
                Outcome::AlreadyPresent => stats.already_present += 1,
                Outcome::Failed => stats.failed += 1,
            }
        }
    }
    Ok(stats)
}

enum Outcome {
    Generated,
    AlreadyPresent,
    Failed,
}

async fn process(ctx: &Ctx, photo: &PhotoRef) -> Outcome {
    match generate(ctx, photo).await {
        Ok(o) => o,
        Err(reason) => {
            tracing::warn!(hash = %photo.hash, %reason, "derivative job failed");
            if let Err(e) = ctx
                .store
                .record_derivative_failure(&photo.hash, &reason)
                .await
            {
                tracing::error!(error = %e, "could not record derivative failure");
            }
            Outcome::Failed
        }
    }
}

async fn generate(ctx: &Ctx, photo: &PhotoRef) -> Result<Outcome, String> {
    // Idempotent: a rebuild against a populated volume only checks filenames.
    let rotation = photo.rotation;
    let (layout, hash) = (ctx.layout.clone(), photo.hash.clone());
    let present = tokio::task::spawn_blocking(move || all_present(&layout, &hash, rotation))
        .await
        .map_err(|e| e.to_string())?;
    if present {
        ctx.store
            .mark_derivatives_ready(&photo.hash, rotation)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(Outcome::AlreadyPresent);
    }

    let source = ctx
        .layout
        .library_path(&photo.rel_path)
        .ok_or("photograph has an invalid path")?;
    let (limits, layout, hash) = (ctx.config.limits, ctx.layout.clone(), photo.hash.clone());
    let job = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let bytes = fsutil::read_capped(&source, limits.max_bytes).map_err(|e| e.to_string())?;
        let info = imagepipe::inspect(&bytes, &limits).map_err(|e| e.to_string())?;
        let derivatives =
            imagepipe::derive(&bytes, &info, &limits, rotation).map_err(|e| e.to_string())?;
        for d in derivatives {
            let path = layout
                .derivative_path(&hash, d.variant, rotation, d.ext)
                .ok_or("invalid hash")?;
            fsutil::write_atomic(&path, &d.bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    });
    // The blocking thread cannot be cancelled, but the job is idempotent, so a
    // straggler that finishes later only writes files that are already valid.
    tokio::time::timeout(ctx.config.decode_timeout, job)
        .await
        .map_err(|_| format!("timed out after {:?}", ctx.config.decode_timeout))?
        .map_err(|e| e.to_string())??;

    ctx.store
        .mark_derivatives_ready(&photo.hash, rotation)
        .await
        .map_err(|e| e.to_string())?;
    // Only now that the database points at the new files is it safe to drop the
    // previous rotation's; a client with an older manifest gets a 404 for them.
    let (layout, hash) = (ctx.layout.clone(), photo.hash.clone());
    tokio::task::spawn_blocking(move || fsutil::prune_other_rotations(&layout, &hash, rotation))
        .await
        .map_err(|e| e.to_string())?;
    Ok(Outcome::Generated)
}

fn all_present(layout: &photoframe_store::Layout, hash: &str, rotation: i32) -> bool {
    VARIANTS.iter().all(|v| {
        layout
            .derivative_path(hash, v.name, rotation, "jpg")
            .is_some_and(|p| p.is_file())
    })
}
