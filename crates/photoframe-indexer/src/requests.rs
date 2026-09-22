//! Work the web tier asked for through the `requests` table.

use photoframe_store::Request;

use crate::ctx::Ctx;
use crate::fsutil;

/// Handle pending requests; returns how many were processed.
pub async fn process(ctx: &Ctx) -> anyhow::Result<usize> {
    let claimed = ctx.store.claim_requests(16).await?;
    let n = claimed.len();
    for req in claimed {
        let outcome = match req.kind.as_str() {
            "delete" => delete(ctx, &req).await.map(|()| None),
            "export" => export(ctx).await.map(Some),
            other => Err(anyhow::anyhow!("unknown request kind {other}")),
        };
        match outcome {
            Ok(result) => {
                ctx.store
                    .finish_request(req.id, true, result.as_deref())
                    .await?
            }
            Err(e) => {
                tracing::error!(id = req.id, kind = %req.kind, error = %e, "request failed");
                ctx.store
                    .finish_request(req.id, false, Some("request failed; see indexer log"))
                    .await?;
            }
        }
    }
    Ok(n)
}

/// Derivatives, then the original, then the row. Every intermediate state is
/// one reconciliation can finish or repair.
async fn delete(ctx: &Ctx, req: &Request) -> anyhow::Result<()> {
    let hash = req.target.clone().unwrap_or_default();
    let entry = ctx
        .store
        .list_index()
        .await?
        .into_iter()
        .find(|e| e.hash == hash);
    let Some(entry) = entry else {
        return Ok(()); // already gone
    };
    let (layout, h) = (ctx.layout.clone(), hash.clone());
    tokio::task::spawn_blocking(move || fsutil::remove_derivatives(&layout, &h)).await?;
    if let Some(path) = ctx.layout.library_path(&entry.rel_path) {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    ctx.store.delete_photo(&hash).await?;
    tracing::info!(%hash, path = %entry.rel_path, "deleted");
    Ok(())
}

/// Write a curation export into `exports/` and return its file name.
async fn export(ctx: &Ctx) -> anyhow::Result<String> {
    let curation = ctx.store.curation_export().await?;
    let stamp: String = curation
        .exported_at
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T' || *c == 'Z')
        .collect();
    let name = format!("curation-{stamp}.json");
    let path = ctx.layout.exports().join(&name);
    let body = serde_json::to_vec_pretty(&curation)?;
    tokio::task::spawn_blocking(move || fsutil::write_atomic(&path, &body)).await??;
    tracing::info!(%name, photos = curation.photos.len(), "curation export written");
    Ok(name)
}

/// Used by the `export` and `import` subcommands.
pub async fn cli_export(ctx: &Ctx, out: &std::path::Path) -> anyhow::Result<()> {
    let curation = ctx.store.curation_export().await?;
    let body = serde_json::to_vec_pretty(&curation)?;
    let out = out.to_path_buf();
    tokio::task::spawn_blocking(move || fsutil::write_atomic(&out, &body)).await??;
    Ok(())
}

pub async fn cli_import(
    ctx: &Ctx,
    input: &std::path::Path,
) -> anyhow::Result<photoframe_store::ImportStats> {
    let body = tokio::fs::read(input).await?;
    let curation: photoframe_store::Curation = serde_json::from_slice(&body)?;
    Ok(ctx.store.curation_import(&curation).await?)
}
