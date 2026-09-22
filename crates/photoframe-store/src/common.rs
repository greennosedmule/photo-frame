//! Query bodies shared by both backends. `async_trait` expands before nested
//! macros do, so the whole `impl Store` is generated here and each backend
//! splices in its own items (migrate, locking) through `$extra`.
//!
//! A backend module must define, before invoking [`impl_store!`]:
//! `ph(&str) -> String` (rewrite `?` placeholders), `ts_in() -> &'static str`
//! (a timestamp bind expression) and `ts_out(&str) -> String` (a timestamp
//! column as RFC 3339 text).

macro_rules! impl_store {
    ($ty:ty, $db:ty; $($extra:tt)*) => {
        impl $ty {
            async fn bump_tx(tx: &mut sqlx::Transaction<'_, $db>) -> $crate::Result<()> {
                sqlx::query(
                    "UPDATE meta SET value = CAST(CAST(value AS BIGINT) + 1 AS TEXT) \
                     WHERE key = 'generation'",
                )
                .execute(&mut **tx)
                .await?;
                Ok(())
            }

            async fn tag_id_tx(
                tx: &mut sqlx::Transaction<'_, $db>,
                name: &str,
            ) -> $crate::Result<(i64, bool)> {
                let found: Option<i64> = sqlx::query_scalar(&ph(
                    "SELECT CAST(id AS BIGINT) FROM tags WHERE lower(name) = lower(?)",
                ))
                .bind(name)
                .fetch_optional(&mut **tx)
                .await?;
                if let Some(id) = found {
                    return Ok((id, false));
                }
                // Another writer may create the same tag between our SELECT and
                // INSERT; ON CONFLICT DO NOTHING covers any unique index, so we
                // just read the winner's row instead of failing.
                let created: Option<i64> = sqlx::query_scalar(&ph(&format!(
                    "INSERT INTO tags (name, created_at) VALUES (?, {}) \
                     ON CONFLICT DO NOTHING RETURNING CAST(id AS BIGINT)",
                    ts_in()
                )))
                .bind(name)
                .bind($crate::now())
                .fetch_optional(&mut **tx)
                .await?;
                if let Some(id) = created {
                    return Ok((id, true));
                }
                let id: i64 = sqlx::query_scalar(&ph(
                    "SELECT CAST(id AS BIGINT) FROM tags WHERE lower(name) = lower(?)",
                ))
                .bind(name)
                .fetch_one(&mut **tx)
                .await?;
                return Ok((id, false));
            }

            async fn photo_exists_tx(
                tx: &mut sqlx::Transaction<'_, $db>,
                hash: &str,
            ) -> $crate::Result<bool> {
                let n: i64 = sqlx::query_scalar(&ph("SELECT COUNT(*) FROM photos WHERE hash = ?"))
                    .bind(hash)
                    .fetch_one(&mut **tx)
                    .await?;
                Ok(n > 0)
            }
        }

        #[async_trait::async_trait]
        impl $crate::Store for $ty {
            $($extra)*

            async fn get_meta(&self, key: &str) -> $crate::Result<Option<String>> {
                Ok(sqlx::query_scalar(&ph("SELECT value FROM meta WHERE key = ?"))
                    .bind(key)
                    .fetch_optional(&self.pool)
                    .await?)
            }

            async fn set_meta(&self, key: &str, value: &str) -> $crate::Result<()> {
                sqlx::query(&ph(
                    "INSERT INTO meta (key, value) VALUES (?, ?) \
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                ))
                .bind(key)
                .bind(value)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn status(&self) -> $crate::Result<$crate::Status> {
                let count = |sql: &'static str| {
                    let pool = self.pool.clone();
                    async move {
                        sqlx::query_scalar::<_, i64>(sql).fetch_one(&pool).await
                    }
                };
                let photo_count = count("SELECT COUNT(*) FROM photos").await?;
                let derivative_queue = count(
                    "SELECT COUNT(*) FROM photos WHERE derivatives_ok = FALSE \
                     OR derivatives_rotation <> rotation",
                )
                .await?;
                let derivative_failures = count(
                    "SELECT COUNT(*) FROM derivative_failures f JOIN photos p ON p.hash = f.photo_hash \
                     WHERE p.derivatives_ok = FALSE OR p.derivatives_rotation <> p.rotation",
                )
                .await?;
                let num = |v: Option<String>| v.and_then(|v| v.parse().ok()).unwrap_or(0i64);
                let state = self.get_meta("indexing_state").await?;
                let last = self.get_meta("last_scan_at").await?.filter(|v| !v.is_empty());
                Ok($crate::Status {
                    generation: self.generation().await?,
                    indexing: state.as_deref().is_some_and(|s| s != "idle"),
                    photo_count,
                    last_scan_at: last,
                    scan_total: num(self.get_meta("scan_total").await?),
                    scan_done: num(self.get_meta("scan_done").await?),
                    derivative_queue,
                    derivative_failures,
                })
            }

            async fn manifest(&self) -> $crate::Result<$crate::Manifest> {
                // Generation first: a concurrent write makes the ETag stale, never ahead.
                let generation = self.generation().await?;
                let indexing = self
                    .get_meta("indexing_state")
                    .await?
                    .is_some_and(|s| s != "idle");
                let eff = "COALESCE(p.date_override, p.taken_at, p.file_mtime)";
                let mut photos: Vec<$crate::ManifestPhoto> = sqlx::query_as(&format!(
                    "SELECT p.hash, \
                       CASE WHEN p.derivatives_rotation IN (90, 270) THEN p.height ELSE p.width END AS width, \
                       CASE WHEN p.derivatives_rotation IN (90, 270) THEN p.width ELSE p.height END AS height, \
                       {} AS effective_date, p.date_source, p.favorite, p.rotation, \
                       p.derivatives_rotation AS media_rotation \
                     FROM photos p WHERE p.derivatives_ok = TRUE AND NOT EXISTS ( \
                       SELECT 1 FROM requests r WHERE r.kind = 'delete' AND r.target = p.hash \
                       AND r.state IN ('pending', 'running')) \
                     ORDER BY {}, p.hash",
                    ts_out(eff),
                    eff
                ))
                .fetch_all(&self.pool)
                .await?;
                let rows: Vec<(String, String)> = sqlx::query_as(
                    "SELECT pt.photo_hash, t.name FROM photo_tags pt \
                     JOIN tags t ON t.id = pt.tag_id ORDER BY lower(t.name), t.name",
                )
                .fetch_all(&self.pool)
                .await?;
                let mut by_photo: std::collections::HashMap<String, Vec<String>> = Default::default();
                for (h, n) in rows {
                    by_photo.entry(h).or_default().push(n);
                }
                let names: Vec<String> = sqlx::query_scalar("SELECT name FROM tags ORDER BY lower(name), name")
                    .fetch_all(&self.pool)
                    .await?;
                let mut counts: std::collections::HashMap<String, i64> = Default::default();
                for p in &mut photos {
                    p.tags = by_photo.remove(&p.hash).unwrap_or_default();
                    for t in &p.tags {
                        *counts.entry(t.clone()).or_default() += 1;
                    }
                }
                let tags = names
                    .into_iter()
                    .map(|name| {
                        let count = counts.get(&name).copied().unwrap_or(0);
                        $crate::TagCount { name, count }
                    })
                    .collect();
                Ok($crate::Manifest { generation, indexing, photos, tags })
            }

            async fn list_tags(&self) -> $crate::Result<Vec<$crate::TagCount>> {
                Ok(sqlx::query_as(
                    "SELECT t.name AS name, COUNT(pt.photo_hash) AS count FROM tags t \
                     LEFT JOIN photo_tags pt ON pt.tag_id = t.id \
                     GROUP BY t.id, t.name ORDER BY lower(t.name), t.name",
                )
                .fetch_all(&self.pool)
                .await?)
            }

            async fn toggle_favorite(&self, hash: &str) -> $crate::Result<Option<bool>> {
                let mut tx = self.begin_tx().await?;
                let v: Option<bool> = sqlx::query_scalar(&ph(
                    "UPDATE photos SET favorite = NOT favorite WHERE hash = ? RETURNING favorite",
                ))
                .bind(hash)
                .fetch_optional(&mut *tx)
                .await?;
                if v.is_some() {
                    Self::bump_tx(&mut tx).await?;
                }
                tx.commit().await?;
                Ok(v)
            }

            async fn create_tag(&self, name: &str) -> $crate::Result<String> {
                let mut tx = self.begin_tx().await?;
                let (id, created) = Self::tag_id_tx(&mut tx, name).await?;
                let canonical: String =
                    sqlx::query_scalar(&ph("SELECT name FROM tags WHERE id = ?"))
                        .bind(id)
                        .fetch_one(&mut *tx)
                        .await?;
                if created {
                    Self::bump_tx(&mut tx).await?;
                }
                tx.commit().await?;
                Ok(canonical)
            }

            async fn add_photo_tag(&self, hash: &str, name: &str) -> $crate::Result<bool> {
                let mut tx = self.begin_tx().await?;
                if !Self::photo_exists_tx(&mut tx, hash).await? {
                    return Ok(false);
                }
                let (id, _) = Self::tag_id_tx(&mut tx, name).await?;
                sqlx::query(&ph(
                    "INSERT INTO photo_tags (photo_hash, tag_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
                ))
                .bind(hash)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                Self::bump_tx(&mut tx).await?;
                tx.commit().await?;
                Ok(true)
            }

            async fn remove_photo_tag(&self, hash: &str, name: &str) -> $crate::Result<bool> {
                let mut tx = self.begin_tx().await?;
                if !Self::photo_exists_tx(&mut tx, hash).await? {
                    return Ok(false);
                }
                let n = sqlx::query(&ph(
                    "DELETE FROM photo_tags WHERE photo_hash = ? AND tag_id IN \
                     (SELECT id FROM tags WHERE lower(name) = lower(?))",
                ))
                .bind(hash)
                .bind(name)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if n > 0 {
                    Self::bump_tx(&mut tx).await?;
                }
                tx.commit().await?;
                Ok(true)
            }

            async fn set_date_override(
                &self,
                hash: &str,
                date: Option<&str>,
            ) -> $crate::Result<bool> {
                let mut tx = self.begin_tx().await?;
                let has_exif: Option<i64> = sqlx::query_scalar(&ph(
                    "SELECT CAST(CASE WHEN taken_at IS NULL THEN 0 ELSE 1 END AS BIGINT) \
                     FROM photos WHERE hash = ?",
                ))
                .bind(hash)
                .fetch_optional(&mut *tx)
                .await?;
                let Some(has_exif) = has_exif else {
                    return Ok(false);
                };
                let source = match (date.is_some(), has_exif != 0) {
                    (true, _) => "override",
                    (false, true) => "exif",
                    (false, false) => "mtime",
                };
                sqlx::query(&ph(&format!(
                    "UPDATE photos SET date_override = {}, date_source = ? WHERE hash = ?",
                    ts_in()
                )))
                .bind(date)
                .bind(source)
                .bind(hash)
                .execute(&mut *tx)
                .await?;
                Self::bump_tx(&mut tx).await?;
                tx.commit().await?;
                Ok(true)
            }

            async fn rotate_photo(&self, hash: &str, delta: i32) -> $crate::Result<Option<i32>> {
                let mut tx = self.begin_tx().await?;
                let current: Option<i32> =
                    sqlx::query_scalar(&ph("SELECT rotation FROM photos WHERE hash = ?"))
                        .bind(hash)
                        .fetch_optional(&mut *tx)
                        .await?;
                let Some(current) = current else {
                    return Ok(None);
                };
                let next = (current + delta).rem_euclid(360);
                sqlx::query(&ph("UPDATE photos SET rotation = ? WHERE hash = ?"))
                    .bind(next)
                    .bind(hash)
                    .execute(&mut *tx)
                    .await?;
                // The manifest's `rotation` changed, so clients must refetch.
                Self::bump_tx(&mut tx).await?;
                tx.commit().await?;
                Ok(Some(next))
            }

            async fn enqueue_request(
                &self,
                kind: $crate::RequestKind,
                target: Option<&str>,
            ) -> $crate::Result<i64> {
                let mut tx = self.begin_tx().await?;
                if kind == $crate::RequestKind::Delete {
                    let ok = match target {
                        Some(t) => Self::photo_exists_tx(&mut tx, t).await?,
                        None => false,
                    };
                    if !ok {
                        return Err($crate::Error::Invalid("no such photograph".into()));
                    }
                }
                let now = $crate::now();
                let id: Option<i64> = sqlx::query_scalar(&ph(&format!(
                    "INSERT INTO requests (kind, target, state, created_at, updated_at) \
                     VALUES (?, ?, 'pending', {0}, {0}) ON CONFLICT DO NOTHING RETURNING id",
                    ts_in()
                )))
                .bind(kind.as_str())
                .bind(target)
                .bind(&now)
                .bind(&now)
                .fetch_optional(&mut *tx)
                .await?;
                let id = match id {
                    Some(id) => {
                        if kind == $crate::RequestKind::Delete {
                            Self::bump_tx(&mut tx).await?;
                        }
                        id
                    }
                    None => {
                        sqlx::query_scalar(&ph(
                            "SELECT id FROM requests WHERE kind = 'delete' AND target = ? \
                             AND state IN ('pending', 'running')",
                        ))
                        .bind(target)
                        .fetch_one(&mut *tx)
                        .await?
                    }
                };
                tx.commit().await?;
                Ok(id)
            }

            async fn get_request(&self, id: i64) -> $crate::Result<Option<$crate::Request>> {
                Ok(sqlx::query_as(&ph(
                    "SELECT id, kind, target, state, result FROM requests WHERE id = ?",
                ))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?)
            }

            async fn list_index(&self) -> $crate::Result<Vec<$crate::IndexEntry>> {
                Ok(sqlx::query_as(&format!(
                    "SELECT hash, rel_path, byte_size, {} AS file_mtime FROM photos",
                    ts_out("file_mtime")
                ))
                .fetch_all(&self.pool)
                .await?)
            }

            async fn photo_exists(&self, hash: &str) -> $crate::Result<bool> {
                let n: i64 = sqlx::query_scalar(&ph("SELECT COUNT(*) FROM photos WHERE hash = ?"))
                    .bind(hash)
                    .fetch_one(&self.pool)
                    .await?;
                Ok(n > 0)
            }

            async fn insert_photo(&self, p: &$crate::NewPhoto) -> $crate::Result<bool> {
                let n = sqlx::query(&ph(&format!(
                    "INSERT INTO photos (hash, rel_path, media_type, mime, byte_size, width, height, \
                       orientation, taken_at, file_mtime, date_source, favorite, derivatives_ok, indexed_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, {0}, {0}, ?, FALSE, FALSE, {0}) \
                     ON CONFLICT (hash) DO NOTHING",
                    ts_in()
                )))
                .bind(&p.hash)
                .bind(&p.rel_path)
                .bind(&p.media_type)
                .bind(&p.mime)
                .bind(p.byte_size)
                .bind(p.width)
                .bind(p.height)
                .bind(p.orientation)
                .bind(&p.taken_at)
                .bind(&p.file_mtime)
                .bind(&p.date_source)
                .bind($crate::now())
                .execute(&self.pool)
                .await?
                .rows_affected();
                Ok(n > 0)
            }

            async fn update_photo_location(
                &self,
                hash: &str,
                rel_path: &str,
                byte_size: i64,
                file_mtime: &str,
            ) -> $crate::Result<()> {
                sqlx::query(&ph(&format!(
                    "UPDATE photos SET rel_path = ?, byte_size = ?, file_mtime = {} WHERE hash = ?",
                    ts_in()
                )))
                .bind(rel_path)
                .bind(byte_size)
                .bind(file_mtime)
                .bind(hash)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn delete_photo(&self, hash: &str) -> $crate::Result<()> {
                let mut tx = self.begin_tx().await?;
                let n = sqlx::query(&ph("DELETE FROM photos WHERE hash = ?"))
                    .bind(hash)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if n > 0 {
                    Self::bump_tx(&mut tx).await?;
                }
                tx.commit().await?;
                Ok(())
            }

            async fn pending_derivatives(
                &self,
                now: &str,
                limit: i64,
            ) -> $crate::Result<Vec<$crate::PhotoRef>> {
                Ok(sqlx::query_as(&ph(&format!(
                    "SELECT p.hash, p.rel_path, p.mime, p.orientation, p.rotation FROM photos p \
                     LEFT JOIN derivative_failures f ON f.photo_hash = p.hash \
                     WHERE (p.derivatives_ok = FALSE OR p.derivatives_rotation <> p.rotation) \
                       AND (f.photo_hash IS NULL OR f.next_retry_at <= {}) \
                     ORDER BY p.indexed_at, p.hash LIMIT ?",
                    ts_in()
                )))
                .bind(now)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?)
            }

            async fn mark_derivatives_ready(&self, hash: &str, rotation: i32) -> $crate::Result<()> {
                let mut tx = self.begin_tx().await?;
                let n = sqlx::query(&ph(
                    "UPDATE photos SET derivatives_ok = TRUE, derivatives_rotation = ? \
                     WHERE hash = ? AND (derivatives_ok = FALSE OR derivatives_rotation <> ?)",
                ))
                .bind(rotation)
                .bind(hash)
                .bind(rotation)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                sqlx::query(&ph("DELETE FROM derivative_failures WHERE photo_hash = ?"))
                    .bind(hash)
                    .execute(&mut *tx)
                    .await?;
                if n > 0 {
                    Self::bump_tx(&mut tx).await?;
                }
                tx.commit().await?;
                Ok(())
            }

            async fn record_derivative_failure(&self, hash: &str, error: &str) -> $crate::Result<i64> {
                let mut tx = self.begin_tx().await?;
                let prev: Option<i64> = sqlx::query_scalar(&ph(
                    "SELECT CAST(attempts AS BIGINT) FROM derivative_failures WHERE photo_hash = ?",
                ))
                .bind(hash)
                .fetch_optional(&mut *tx)
                .await?;
                let attempts = prev.unwrap_or(0) + 1;
                // 1 min, 2, 4 ... capped at a day.
                let delay = (60i64 << attempts.clamp(1, 12).saturating_sub(1)).min(86_400);
                let next = $crate::ts_from_unix(time::OffsetDateTime::now_utc().unix_timestamp() + delay);
                sqlx::query(&ph(&format!(
                    "INSERT INTO derivative_failures (photo_hash, attempts, last_error, next_retry_at) \
                     VALUES (?, ?, ?, {}) ON CONFLICT (photo_hash) DO UPDATE SET \
                     attempts = excluded.attempts, last_error = excluded.last_error, \
                     next_retry_at = excluded.next_retry_at",
                    ts_in()
                )))
                .bind(hash)
                .bind(attempts)
                .bind(error)
                .bind(next)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(attempts)
            }

            async fn claim_requests(&self, limit: i64) -> $crate::Result<Vec<$crate::Request>> {
                let mut tx = self.begin_tx().await?;
                let mut rows: Vec<$crate::Request> = sqlx::query_as(&ph(
                    "SELECT id, kind, target, state, result FROM requests \
                     WHERE state = 'pending' ORDER BY id LIMIT ?",
                ))
                .bind(limit)
                .fetch_all(&mut *tx)
                .await?;
                for r in &mut rows {
                    sqlx::query(&ph(&format!(
                        "UPDATE requests SET state = 'running', updated_at = {} WHERE id = ?",
                        ts_in()
                    )))
                    .bind($crate::now())
                    .bind(r.id)
                    .execute(&mut *tx)
                    .await?;
                    r.state = "running".into();
                }
                tx.commit().await?;
                Ok(rows)
            }

            async fn finish_request(
                &self,
                id: i64,
                ok: bool,
                result: Option<&str>,
            ) -> $crate::Result<()> {
                sqlx::query(&ph(&format!(
                    "UPDATE requests SET state = ?, result = ?, updated_at = {} WHERE id = ?",
                    ts_in()
                )))
                .bind(if ok { "done" } else { "failed" })
                .bind(result)
                .bind($crate::now())
                .bind(id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn reset_running_requests(&self) -> $crate::Result<()> {
                sqlx::query("UPDATE requests SET state = 'pending' WHERE state = 'running'")
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn curation_export(&self) -> $crate::Result<$crate::Curation> {
                let tags: Vec<String> = sqlx::query_scalar("SELECT name FROM tags ORDER BY lower(name), name")
                    .fetch_all(&self.pool)
                    .await?;
                let rows: Vec<(String, bool, Option<String>, i32)> = sqlx::query_as(&format!(
                    "SELECT hash, favorite, {}, rotation FROM photos WHERE favorite = TRUE \
                     OR date_override IS NOT NULL OR rotation <> 0 \
                     OR EXISTS (SELECT 1 FROM photo_tags pt WHERE pt.photo_hash = photos.hash) \
                     ORDER BY hash",
                    ts_out("date_override")
                ))
                .fetch_all(&self.pool)
                .await?;
                let mut photos: std::collections::BTreeMap<String, $crate::CuratedPhoto> = rows
                    .into_iter()
                    .map(|(h, favorite, date_override, rotation)| {
                        (
                            h,
                            $crate::CuratedPhoto { favorite, date_override, rotation, tags: vec![] },
                        )
                    })
                    .collect();
                let tag_rows: Vec<(String, String)> = sqlx::query_as(
                    "SELECT pt.photo_hash, t.name FROM photo_tags pt \
                     JOIN tags t ON t.id = pt.tag_id ORDER BY lower(t.name), t.name",
                )
                .fetch_all(&self.pool)
                .await?;
                for (h, n) in tag_rows {
                    if let Some(p) = photos.get_mut(&h) {
                        p.tags.push(n);
                    }
                }
                Ok($crate::Curation { version: 1, exported_at: $crate::now(), tags, photos })
            }

            async fn curation_import(&self, c: &$crate::Curation) -> $crate::Result<$crate::ImportStats> {
                if c.version != 1 {
                    return Err($crate::Error::Invalid(format!(
                        "unsupported export version {}",
                        c.version
                    )));
                }
                let mut stats = $crate::ImportStats::default();
                let mut tx = self.begin_tx().await?;
                for name in &c.tags {
                    Self::tag_id_tx(&mut tx, name).await?;
                }
                for (hash, cp) in &c.photos {
                    if !Self::photo_exists_tx(&mut tx, hash).await? {
                        stats.skipped_missing += 1;
                        continue;
                    }
                    if cp.favorite {
                        sqlx::query(&ph("UPDATE photos SET favorite = TRUE WHERE hash = ?"))
                            .bind(hash)
                            .execute(&mut *tx)
                            .await?;
                    }
                    if cp.rotation != 0 {
                        if !matches!(cp.rotation, 90 | 180 | 270) {
                            return Err($crate::Error::Invalid(format!(
                                "bad rotation {} for {hash}",
                                cp.rotation
                            )));
                        }
                        sqlx::query(&ph("UPDATE photos SET rotation = ? WHERE hash = ?"))
                            .bind(cp.rotation)
                            .bind(hash)
                            .execute(&mut *tx)
                            .await?;
                    }
                    if let Some(d) = &cp.date_override {
                        let d = $crate::normalize_ts(d).ok_or_else(|| {
                            $crate::Error::Invalid(format!("bad date_override for {hash}"))
                        })?;
                        sqlx::query(&ph(&format!(
                            "UPDATE photos SET date_override = {}, date_source = 'override' WHERE hash = ?",
                            ts_in()
                        )))
                        .bind(d)
                        .bind(hash)
                        .execute(&mut *tx)
                        .await?;
                    }
                    for name in &cp.tags {
                        let (id, _) = Self::tag_id_tx(&mut tx, name).await?;
                        sqlx::query(&ph(
                            "INSERT INTO photo_tags (photo_hash, tag_id) VALUES (?, ?) \
                             ON CONFLICT DO NOTHING",
                        ))
                        .bind(hash)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    stats.applied += 1;
                }
                Self::bump_tx(&mut tx).await?;
                tx.commit().await?;
                Ok(stats)
            }
        }
    };
}
