//! Promotion from `incoming/` into `library/YYYY/MM/`.

use std::path::Path;

use photoframe_store::{NewPhoto, normalize_ts, ts_from_unix};

use crate::ctx::Ctx;
use crate::fsutil::{self, Found};
use crate::scan::upright_dims;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PromoteStats {
    pub promoted: usize,
    pub duplicates: usize,
    pub quarantined: usize,
    pub too_new: usize,
}

enum Outcome {
    Promoted,
    Duplicate,
    Quarantined,
}

pub async fn promote(ctx: &Ctx) -> anyhow::Result<PromoteStats> {
    let mut stats = PromoteStats::default();
    let root = ctx.layout.incoming();
    let files = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || fsutil::walk(&root)).await?
    };
    let files = match files {
        Ok(w) => w.files,
        // No incoming/ yet is normal on a fresh volume.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(stats),
        Err(e) => return Err(e.into()),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let quiet = ctx.config.ingest_quiet.as_secs() as i64;

    for f in files {
        if ctx.stopping() {
            break;
        }
        // A file still being written keeps moving its mtime.
        if now - f.mtime < quiet {
            stats.too_new += 1;
            continue;
        }
        let outcome = promote_one(ctx, &f).await;
        if outcome.is_ok() {
            fsutil::prune_empty_parent(&f.abs_path, &root);
        }
        match outcome {
            Ok(Outcome::Promoted) => stats.promoted += 1,
            Ok(Outcome::Duplicate) => stats.duplicates += 1,
            Ok(Outcome::Quarantined) => stats.quarantined += 1,
            Err(e) => {
                tracing::error!(path = %f.rel_path, error = %e, "promotion failed; will retry")
            }
        }
    }
    Ok(stats)
}

async fn promote_one(ctx: &Ctx, f: &Found) -> anyhow::Result<Outcome> {
    let max_upload = ctx.config.max_upload_bytes;
    let limits = ctx.config.limits;
    let path = f.abs_path.clone();

    // Size, magic bytes and pixel limits are all checked before any decode.
    let checked = tokio::task::spawn_blocking(move || {
        if std::fs::metadata(&path)?.len() > max_upload {
            return Ok(Err(format!("larger than MAX_UPLOAD_BYTES ({max_upload})")));
        }
        let bytes = fsutil::read_capped(&path, limits.max_bytes.max(max_upload))?;
        Ok::<_, std::io::Error>(match imagepipe::inspect(&bytes, &limits) {
            Ok(info) => Ok((fsutil::hash_bytes(&bytes), info)),
            Err(e) => Err(e.to_string()),
        })
    })
    .await??;

    let (hash, info) = match checked {
        Ok(v) => v,
        Err(reason) => {
            quarantine(ctx, &f.abs_path, &reason).await?;
            return Ok(Outcome::Quarantined);
        }
    };

    if ctx.store.photo_exists(&hash).await? {
        tokio::fs::remove_file(&f.abs_path).await?;
        tracing::info!(path = %f.rel_path, "duplicate of an existing photograph, removed");
        return Ok(Outcome::Duplicate);
    }

    // Filed by effective date; `date_override` cannot exist for a new photograph.
    let mtime = ts_from_unix(f.mtime);
    let date = info.taken_at.clone().unwrap_or_else(|| mtime.clone());
    let date = normalize_ts(&date).unwrap_or(mtime.clone());
    let (year, month) = (&date[0..4], &date[5..7]);
    let stem = sanitize_stem(&f.abs_path);
    let ext = ext_for(info.format);
    let dir = ctx.layout.library().join(year).join(month);
    let dest = fsutil::free_name(&dir, &stem, ext);
    let (from, to) = (f.abs_path.clone(), dest.clone());
    tokio::task::spawn_blocking(move || fsutil::move_file(&from, &to)).await??;

    // Index straight away so the photograph does not wait for the next walk.
    // If we die between the move and the insert, reconciliation finds the file.
    let md = tokio::fs::metadata(&dest).await?;
    let rel_path = dest
        .strip_prefix(ctx.layout.library())?
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let (width, height) = upright_dims(&info);
    ctx.store
        .insert_photo(&NewPhoto {
            hash,
            rel_path: rel_path.clone(),
            media_type: "image".into(),
            mime: info.format.mime().into(),
            byte_size: md.len() as i64,
            width: i32::try_from(width).unwrap_or(i32::MAX),
            height: i32::try_from(height).unwrap_or(i32::MAX),
            orientation: i32::from(info.orientation),
            date_source: if info.taken_at.is_some() {
                "exif"
            } else {
                "mtime"
            }
            .into(),
            taken_at: info.taken_at,
            file_mtime: ts_from_unix(fsutil::mtime_secs(&md)),
        })
        .await?;
    tracing::info!(to = %rel_path, "promoted");
    Ok(Outcome::Promoted)
}

/// Move to `quarantine/` with a sibling `.reason.txt`. Never deletes.
async fn quarantine(ctx: &Ctx, path: &Path, reason: &str) -> anyhow::Result<()> {
    let dir = ctx.layout.quarantine();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let (from, reason) = (path.to_path_buf(), reason.to_string());
    let logged = reason.clone();
    let dest = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
            _ => (name.clone(), "bin".to_string()),
        };
        let dest = fsutil::free_name(&dir, &stem, &ext);
        fsutil::move_file(&from, &dest)?;
        let mut note = dest.clone().into_os_string();
        note.push(".reason.txt");
        std::fs::write(note, format!("{reason}\n"))?;
        Ok(dest)
    })
    .await??;
    tracing::warn!(file = %dest.display(), reason = %logged, "quarantined");
    Ok(())
}

/// The original file name without extension, reduced to characters that are
/// safe on every filesystem the volume might be mounted from.
fn sanitize_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "photo".into()
    } else {
        cleaned.chars().take(100).collect()
    }
}

/// Extension follows the detected format, never the uploaded name.
fn ext_for(f: imagepipe::Format) -> &'static str {
    match f {
        imagepipe::Format::Jpeg => "jpg",
        imagepipe::Format::Png => "png",
        imagepipe::Format::WebP => "webp",
        imagepipe::Format::Heic => "heic",
        imagepipe::Format::Avif => "avif",
    }
}
