//! Reconciliation: make the database agree with `library/`.
//!
//! The filesystem decides which photographs exist; the database only decides
//! curation. Nothing here writes to `library/`.

use std::collections::{HashMap, HashSet};

use anyhow::Context as _;
use photoframe_store::{IndexEntry, NewPhoto, ts_from_unix, unix_of};

use crate::ctx::Ctx;
use crate::fsutil::{self, Found};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanStats {
    pub seen: usize,
    pub added: usize,
    pub moved: usize,
    pub removed: usize,
    pub skipped: usize,
}

/// State kept between scans so unreadable or unsupported files are not
/// re-hashed every minute.
#[derive(Default)]
pub struct Scanner {
    skipped: HashMap<String, (i64, i64)>,
    empty_strikes: u32,
}

impl Scanner {
    pub async fn reconcile(&mut self, ctx: &Ctx) -> anyhow::Result<ScanStats> {
        let root = ctx.layout.library();
        let walk = {
            let root = root.clone();
            tokio::task::spawn_blocking(move || fsutil::walk(&root)).await?
        }
        .with_context(|| format!("cannot walk {}", root.display()))?;

        let mut stats = ScanStats::default();
        let total = walk.files.len();
        ctx.store.set_meta("scan_total", &total.to_string()).await?;
        ctx.store.set_meta("scan_done", "0").await?;

        let index = ctx.store.list_index().await?;
        let by_path: HashMap<String, IndexEntry> = index
            .iter()
            .map(|e| (e.rel_path.clone(), e.clone()))
            .collect();
        let mut by_hash: HashMap<String, IndexEntry> =
            index.iter().map(|e| (e.hash.clone(), e.clone())).collect();
        let present: HashSet<&str> = walk.files.iter().map(|f| f.rel_path.as_str()).collect();
        let mut live: HashSet<String> = HashSet::new();
        let mut gone: HashSet<String> = HashSet::new();

        for (i, f) in walk.files.iter().enumerate() {
            if ctx.stopping() {
                return Ok(stats);
            }
            if i % 50 == 0 {
                ctx.store.set_meta("scan_done", &i.to_string()).await?;
            }
            stats.seen += 1;

            // Fast path: same path, size and mtime as the indexed row.
            if let Some(e) = by_path.get(&f.rel_path)
                && e.byte_size == f.size
                && unix_of(&e.file_mtime) == Some(f.mtime)
            {
                live.insert(e.hash.clone());
                continue;
            }
            if self.skipped.get(&f.rel_path) == Some(&(f.size, f.mtime)) {
                stats.skipped += 1;
                continue;
            }

            let path = f.abs_path.clone();
            let hash = match tokio::task::spawn_blocking(move || fsutil::hash_file(&path)).await? {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(path = %f.rel_path, error = %e, "cannot read file");
                    continue;
                }
            };

            if let Some(known) = by_hash.get(&hash).cloned() {
                if known.rel_path == f.rel_path {
                    // Same content, touched: refresh the stat so the fast path applies again.
                    self.relocate(ctx, &known.hash, f).await?;
                    live.insert(hash);
                } else if present.contains(known.rel_path.as_str()) && !gone.contains(&hash) {
                    // A second copy of an indexed photograph. The first one wins.
                    tracing::info!(path = %f.rel_path, original = %known.rel_path, "duplicate content, ignoring copy");
                    self.skipped.insert(f.rel_path.clone(), (f.size, f.mtime));
                    stats.skipped += 1;
                } else {
                    // Moved or renamed: identity is content, so curation follows it.
                    tracing::info!(from = %known.rel_path, to = %f.rel_path, "photograph moved");
                    self.relocate(ctx, &known.hash, f).await?;
                    by_hash.insert(
                        hash.clone(),
                        IndexEntry {
                            rel_path: f.rel_path.clone(),
                            ..known
                        },
                    );
                    live.insert(hash);
                    stats.moved += 1;
                }
                continue;
            }

            // New content. If the path held a different photograph, that one was
            // replaced (re-encoding is a new photograph, not an edit).
            if let Some(old) = by_path.get(&f.rel_path)
                && !gone.contains(&old.hash)
                && !live.contains(&old.hash)
            {
                self.drop_photo(ctx, &old.hash).await?;
                gone.insert(old.hash.clone());
                by_hash.remove(&old.hash);
                stats.removed += 1;
            }
            match self.index_new(ctx, f, &hash).await {
                Ok(true) => {
                    by_hash.insert(
                        hash.clone(),
                        IndexEntry {
                            hash: hash.clone(),
                            rel_path: f.rel_path.clone(),
                            byte_size: f.size,
                            file_mtime: ts_from_unix(f.mtime),
                        },
                    );
                    live.insert(hash.clone());
                    stats.added += 1;
                }
                Ok(false) => {}
                Err(reason) => {
                    tracing::warn!(path = %f.rel_path, %reason, "not indexing file");
                    self.skipped.insert(f.rel_path.clone(), (f.size, f.mtime));
                    stats.skipped += 1;
                }
            }
        }

        // Rows whose file vanished. Never conclude that from a partial or
        // suspiciously empty listing: an unmounted NFS volume looks like this.
        let suspicious = walk.incomplete || (walk.files.is_empty() && !index.is_empty());
        if suspicious && !walk.incomplete {
            self.empty_strikes += 1;
        } else {
            self.empty_strikes = 0;
        }
        if walk.incomplete || (suspicious && self.empty_strikes < 2) {
            if suspicious {
                tracing::warn!("listing looks incomplete or empty; not deleting rows this scan");
            }
        } else {
            for e in &index {
                if !live.contains(&e.hash) && !gone.contains(&e.hash) {
                    tracing::info!(path = %e.rel_path, "file vanished, removing row");
                    self.drop_photo(ctx, &e.hash).await?;
                    stats.removed += 1;
                }
            }
        }
        ctx.store.set_meta("scan_done", &total.to_string()).await?;
        // Forget skip entries for paths that no longer exist.
        self.skipped.retain(|p, _| present.contains(p.as_str()));
        Ok(stats)
    }

    async fn relocate(&self, ctx: &Ctx, hash: &str, f: &Found) -> anyhow::Result<()> {
        ctx.store
            .update_photo_location(hash, &f.rel_path, f.size, &ts_from_unix(f.mtime))
            .await?;
        Ok(())
    }

    async fn drop_photo(&self, ctx: &Ctx, hash: &str) -> anyhow::Result<()> {
        let (layout, h) = (ctx.layout.clone(), hash.to_string());
        tokio::task::spawn_blocking(move || fsutil::remove_derivatives(&layout, &h)).await?;
        ctx.store.delete_photo(hash).await?;
        Ok(())
    }

    /// `Ok(true)` indexed, `Ok(false)` file changed underneath us (retry next
    /// scan), `Err(reason)` not a usable photograph.
    async fn index_new(&self, ctx: &Ctx, f: &Found, hash: &str) -> Result<bool, String> {
        let (path, limits) = (f.abs_path.clone(), ctx.config.limits);
        let (bytes_hash, info) = tokio::task::spawn_blocking(move || {
            let bytes = fsutil::read_capped(&path, limits.max_bytes).map_err(|e| e.to_string())?;
            let info = imagepipe::inspect(&bytes, &limits).map_err(|e| e.to_string())?;
            Ok::<_, String>((fsutil::hash_bytes(&bytes), info))
        })
        .await
        .map_err(|e| e.to_string())??;
        if bytes_hash != hash {
            return Ok(false);
        }
        // Dimensions are stored upright, the way derivatives come out.
        let (width, height) = upright_dims(&info);
        let photo = NewPhoto {
            hash: hash.to_string(),
            rel_path: f.rel_path.clone(),
            media_type: "image".into(),
            mime: info.format.mime().into(),
            byte_size: f.size,
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
            file_mtime: ts_from_unix(f.mtime),
        };
        ctx.store
            .insert_photo(&photo)
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    }
}

/// Header dimensions with EXIF orientation applied. HEIC decoding applies its
/// own rotation, so its probe already reports upright dimensions.
pub fn upright_dims(info: &imagepipe::Info) -> (u32, u32) {
    if info.orientation >= 5 && info.format != imagepipe::Format::Heic {
        (info.height, info.width)
    } else {
        (info.width, info.height)
    }
}
