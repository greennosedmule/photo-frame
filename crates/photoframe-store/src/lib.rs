//! Database access behind a narrow [`Store`] trait with SQLite and Postgres
//! implementations, selected at startup from `DATABASE_URL`.
//!
//! sqlx's compile-time macros are backend-specific, so queries live inside each
//! implementation. Keep this trait small so the duplication stays bounded.

#[macro_use]
mod common;
mod error;
mod layout;
mod model;
mod postgres;
mod sqlite;

use std::sync::Arc;

use async_trait::async_trait;

pub use error::{Error, Result};
pub use layout::{Layout, is_hash};
pub use model::*;

/// Which backend a `DATABASE_URL` selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

impl Backend {
    pub fn from_url(url: &str) -> Result<Self> {
        if url.starts_with("sqlite:") {
            Ok(Self::Sqlite)
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Ok(Self::Postgres)
        } else {
            Err(Error::UnsupportedUrl)
        }
    }
}

/// Held by the indexer for its lifetime. Dropping it does not release the lock
/// reliably (release is async); call [`Store::release_indexer_lock`] on shutdown.
#[derive(Debug)]
pub struct IndexerLock {
    _private: (),
}

impl IndexerLock {
    fn new() -> Self {
        Self { _private: () }
    }
}

#[async_trait]
pub trait Store: Send + Sync {
    /// Apply embedded migrations. Idempotent and safe to run concurrently.
    async fn migrate(&self) -> Result<()>;

    /// Current value of the `generation` counter in `meta`.
    async fn generation(&self) -> Result<i64>;

    /// Increment the `generation` counter and return the new value.
    async fn bump_generation(&self) -> Result<i64>;

    /// Set by `POST /api/scan`; the indexer picks it up on its next tick.
    async fn request_scan(&self) -> Result<()>;

    /// Read and clear the scan-request flag. True if a scan was requested.
    async fn take_scan_request(&self) -> Result<bool>;

    /// Take the indexer singleton lock. Fails with [`Error::LockHeld`] when another
    /// instance holds it, so a misconfigured second indexer exits loudly.
    async fn acquire_indexer_lock(&self) -> Result<IndexerLock>;

    async fn release_indexer_lock(&self, lock: IndexerLock) -> Result<()>;

    // ---- shared state ------------------------------------------------------

    async fn get_meta(&self, key: &str) -> Result<Option<String>>;
    async fn set_meta(&self, key: &str, value: &str) -> Result<()>;

    /// Status for `/api/status` and the management panel.
    async fn status(&self) -> Result<Status>;

    // ---- web: read ---------------------------------------------------------

    /// Photographs with `derivatives_ok` and no in-flight delete, plus tag counts.
    async fn manifest(&self) -> Result<Manifest>;

    async fn list_tags(&self) -> Result<Vec<TagCount>>;

    // ---- web: shared-state mutations (each bumps `generation`) -------------

    /// Toggle favourite. `None` if no such photograph.
    async fn toggle_favorite(&self, hash: &str) -> Result<Option<bool>>;

    /// Create a tag if absent; returns the canonical stored name.
    async fn create_tag(&self, name: &str) -> Result<String>;

    /// Attach a tag (creating it if needed). False if no such photograph.
    async fn add_photo_tag(&self, hash: &str, name: &str) -> Result<bool>;

    /// Detach a tag. False if no such photograph.
    async fn remove_photo_tag(&self, hash: &str, name: &str) -> Result<bool>;

    /// Set or clear `date_override` (RFC 3339). False if no such photograph.
    async fn set_date_override(&self, hash: &str, date: Option<&str>) -> Result<bool>;

    /// Turn a photograph by `delta` degrees clockwise (a multiple of 90).
    /// Relative so concurrent and batched rotations compose. Returns the new
    /// rotation, or `None` if no such photograph. The indexer regenerates the
    /// derivatives; until then the manifest keeps serving the old ones.
    async fn rotate_photo(&self, hash: &str, delta: i32) -> Result<Option<i32>>;

    // ---- web -> indexer requests -------------------------------------------

    /// Record a delete or export request. A delete already in flight for the
    /// same photograph returns that request's id. A delete request for an
    /// unknown photograph is refused with [`Error::Invalid`].
    async fn enqueue_request(&self, kind: RequestKind, target: Option<&str>) -> Result<i64>;
    async fn get_request(&self, id: i64) -> Result<Option<Request>>;

    // ---- indexer -----------------------------------------------------------

    async fn list_index(&self) -> Result<Vec<IndexEntry>>;
    async fn photo_exists(&self, hash: &str) -> Result<bool>;

    /// Insert a new photograph. Returns false if the hash was already present.
    async fn insert_photo(&self, photo: &NewPhoto) -> Result<bool>;

    /// A known photograph moved or its stat changed.
    async fn update_photo_location(
        &self,
        hash: &str,
        rel_path: &str,
        byte_size: i64,
        file_mtime: &str,
    ) -> Result<()>;

    /// Delete the row (tags and failures cascade).
    async fn delete_photo(&self, hash: &str) -> Result<()>;

    /// Photographs still needing derivatives and not backing off at `now`.
    async fn pending_derivatives(&self, now: &str, limit: i64) -> Result<Vec<PhotoRef>>;
    /// Derivatives now exist for `rotation`. Also clears any recorded failure.
    /// If the curated rotation moved meanwhile the photograph stays pending.
    async fn mark_derivatives_ready(&self, hash: &str, rotation: i32) -> Result<()>;
    /// Record a failure: increments attempts and schedules the next retry with
    /// exponential backoff. Returns the new attempt count.
    async fn record_derivative_failure(&self, hash: &str, error: &str) -> Result<i64>;

    /// Pending requests, oldest first, marked `running` as they are returned.
    async fn claim_requests(&self, limit: i64) -> Result<Vec<Request>>;
    async fn finish_request(&self, id: i64, ok: bool, result: Option<&str>) -> Result<()>;
    /// Crash recovery: `running` requests go back to `pending`.
    async fn reset_running_requests(&self) -> Result<()>;

    // ---- curation ----------------------------------------------------------

    async fn curation_export(&self) -> Result<Curation>;
    /// Merge only: adds tags, sets favourites and overrides, never clears.
    async fn curation_import(&self, c: &Curation) -> Result<ImportStats>;
}

/// Connect to the backend named by `url` and return it as a trait object.
/// Does not migrate; callers run [`Store::migrate`] explicitly.
pub async fn connect(url: &str) -> Result<Arc<dyn Store>> {
    match Backend::from_url(url)? {
        Backend::Sqlite => Ok(Arc::new(sqlite::SqliteStore::connect(url).await?)),
        Backend::Postgres => Ok(Arc::new(postgres::PostgresStore::connect(url).await?)),
    }
}

#[cfg(test)]
mod tests;
