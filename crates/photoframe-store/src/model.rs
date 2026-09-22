//! Backend-neutral data types crossing the [`crate::Store`] boundary.
//! Timestamps are RFC 3339 UTC strings (`2026-07-04T18:22:41Z`) on both backends.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// A photograph the indexer has just extracted metadata for.
#[derive(Debug, Clone)]
pub struct NewPhoto {
    pub hash: String,
    pub rel_path: String,
    pub media_type: String,
    pub mime: String,
    pub byte_size: i64,
    pub width: i32,
    pub height: i32,
    pub orientation: i32,
    pub taken_at: Option<String>,
    pub file_mtime: String,
    pub date_source: String,
}

/// What reconciliation needs to decide whether a file changed.
#[derive(Debug, Clone, FromRow)]
pub struct IndexEntry {
    pub hash: String,
    pub rel_path: String,
    pub byte_size: i64,
    pub file_mtime: String,
}

/// What derivative generation needs.
#[derive(Debug, Clone, FromRow)]
pub struct PhotoRef {
    pub hash: String,
    pub rel_path: String,
    pub mime: String,
    pub orientation: i32,
    /// Curated rotation to apply on top of EXIF orientation.
    pub rotation: i32,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ManifestPhoto {
    pub hash: String,
    #[sqlx(rename = "width")]
    pub w: i32,
    #[sqlx(rename = "height")]
    pub h: i32,
    pub effective_date: String,
    pub date_source: String,
    pub favorite: bool,
    /// Curated rotation, degrees clockwise: what the photograph should show.
    pub rotation: i32,
    /// The rotation the derivatives currently on disk were made for. Media URLs
    /// use this; it catches up to `rotation` once the indexer regenerates. `w`
    /// and `h` already reflect it.
    pub media_rotation: i32,
    #[sqlx(skip)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub generation: i64,
    pub indexing: bool,
    pub photos: Vec<ManifestPhoto>,
    pub tags: Vec<TagCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub generation: i64,
    pub indexing: bool,
    pub photo_count: i64,
    pub last_scan_at: Option<String>,
    pub scan_total: i64,
    pub scan_done: i64,
    pub derivative_queue: i64,
    pub derivative_failures: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    Delete,
    Export,
}

impl RequestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Export => "export",
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Request {
    pub id: i64,
    pub kind: String,
    pub target: Option<String>,
    pub state: String,
    pub result: Option<String>,
}

/// Curation export, format version 1. The only state a rescan cannot rebuild.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Curation {
    pub version: u32,
    pub exported_at: String,
    pub tags: Vec<String>,
    pub photos: BTreeMap<String, CuratedPhoto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CuratedPhoto {
    #[serde(default)]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_override: Option<String>,
    /// Degrees clockwise: 0, 90, 180 or 270. Absent means 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: i32,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportStats {
    pub applied: u64,
    pub skipped_missing: u64,
}

pub fn now() -> String {
    format_utc(OffsetDateTime::now_utc())
}

pub fn format_utc(t: OffsetDateTime) -> String {
    let t = t
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .unwrap_or(t);
    t.format(&Rfc3339).unwrap_or_default()
}

/// Parse any RFC 3339 timestamp and re-emit it as UTC with a `Z` suffix, the
/// one form that sorts correctly as text on SQLite.
pub fn normalize_ts(s: &str) -> Option<String> {
    OffsetDateTime::parse(s.trim(), &Rfc3339)
        .ok()
        .map(format_utc)
}

/// Unix seconds, for callers that only have a file mtime.
pub fn ts_from_unix(secs: i64) -> String {
    OffsetDateTime::from_unix_timestamp(secs)
        .map(format_utc)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// Seconds since the epoch of a normalised timestamp.
pub fn unix_of(s: &str) -> Option<i64> {
    OffsetDateTime::parse(s, &Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}
