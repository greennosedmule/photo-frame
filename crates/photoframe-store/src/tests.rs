//! Backend-agnostic contract tests. SQLite always runs; Postgres runs when
//! `TEST_POSTGRES_URL` is set (CI provides a service container).

use crate::{Curation, Error, NewPhoto, RequestKind, Store, connect};

/// A URL for a freshly created, empty database on the server named by
/// `TEST_POSTGRES_URL`, or `None` when that is unset. Each test gets its own
/// so they neither run into each other in parallel nor inherit leftovers.
/// The databases (`pf_test_*`) are not dropped; a throwaway server is assumed.
async fn fresh_postgres() -> Option<String> {
    use sqlx::Connection;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    let base = std::env::var("TEST_POSTGRES_URL").ok()?;
    let (head, query) = base
        .split_once('?')
        .map_or((base.as_str(), None), |(h, q)| (h, Some(q)));
    let server = &head[..head.rfind('/').expect("TEST_POSTGRES_URL has a database")];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!(
        "pf_test_{}_{}_{nanos}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    let mut admin = sqlx::PgConnection::connect(&base).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut admin)
        .await
        .unwrap();
    Some(match query {
        Some(q) => format!("{server}/{name}?{q}"),
        None => format!("{server}/{name}"),
    })
}

async fn generation_and_scan_flag(store: &dyn Store) {
    store.migrate().await.unwrap();
    store.migrate().await.unwrap(); // idempotent

    let g = store.generation().await.unwrap();
    assert_eq!(store.bump_generation().await.unwrap(), g + 1);
    assert_eq!(store.generation().await.unwrap(), g + 1);

    let _ = store.take_scan_request().await.unwrap();
    assert!(!store.take_scan_request().await.unwrap());
    store.request_scan().await.unwrap();
    assert!(store.take_scan_request().await.unwrap());
    assert!(!store.take_scan_request().await.unwrap());
}

async fn lock_is_exclusive(store: &dyn Store, second: &dyn Store) {
    let lock = store.acquire_indexer_lock().await.unwrap();
    assert!(matches!(
        second.acquire_indexer_lock().await,
        Err(Error::LockHeld)
    ));
    store.release_indexer_lock(lock).await.unwrap();
    let again = second.acquire_indexer_lock().await.unwrap();
    second.release_indexer_lock(again).await.unwrap();
}

#[tokio::test]
async fn sqlite_contract() {
    let dir = std::env::temp_dir().join(format!("pf-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite://{}/t.db", dir.display());
    let a = connect(&url).await.unwrap();
    let b = connect(&url).await.unwrap();
    generation_and_scan_flag(a.as_ref()).await;
    lock_is_exclusive(a.as_ref(), b.as_ref()).await;
    photos_curation_requests(a.as_ref()).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_contract() {
    let Some(url) = fresh_postgres().await else {
        eprintln!("TEST_POSTGRES_URL not set; skipping");
        return;
    };
    let a = connect(&url).await.unwrap();
    let b = connect(&url).await.unwrap();
    generation_and_scan_flag(a.as_ref()).await;
    lock_is_exclusive(a.as_ref(), b.as_ref()).await;
    photos_curation_requests(a.as_ref()).await;
}

#[test]
fn rejects_unknown_scheme() {
    assert!(crate::Backend::from_url("mysql://x").is_err());
}

#[tokio::test]
async fn sqlite_migration_0002_objects() {
    use sqlx::sqlite::SqlitePoolOptions;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../../migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    for t in ["requests", "derivative_failures"] {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
                .bind(t)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 1, "{t} missing");
    }
    let keys: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM meta WHERE key IN ('last_scan_at','indexing_state','scan_total','scan_done')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(keys, 4);
}

#[tokio::test]
async fn sqlite_concurrent_migrate() {
    let dir = std::env::temp_dir().join(format!("pf-store-race-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite://{}/t.db", dir.display());
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            connect(&url).await.unwrap().migrate().await
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn photo(hash: &str, path: &str, taken: Option<&str>) -> NewPhoto {
    NewPhoto {
        hash: hash.into(),
        rel_path: path.into(),
        media_type: "image".into(),
        mime: "image/jpeg".into(),
        byte_size: 100,
        width: 4000,
        height: 3000,
        orientation: 1,
        taken_at: taken.map(Into::into),
        file_mtime: "2020-01-01T00:00:00Z".into(),
        date_source: if taken.is_some() { "exif" } else { "mtime" }.into(),
    }
}

/// Wipe rows so the contract can run against a shared Postgres database.
async fn reset(store: &dyn Store) {
    for h in ["h1", "h2", "h3"] {
        store.delete_photo(h).await.unwrap();
    }
}

async fn photos_curation_requests(store: &dyn Store) {
    store.migrate().await.unwrap();
    reset(store).await;

    assert!(
        store
            .insert_photo(&photo("h1", "2024/a.jpg", Some("2024-05-01T10:00:00Z")))
            .await
            .unwrap()
    );
    assert!(
        !store
            .insert_photo(&photo("h1", "2024/a.jpg", None))
            .await
            .unwrap()
    );
    assert!(
        store
            .insert_photo(&photo("h2", "2024/b.jpg", None))
            .await
            .unwrap()
    );
    assert!(store.photo_exists("h1").await.unwrap());
    let idx = store.list_index().await.unwrap();
    assert_eq!(idx.len(), 2);
    assert_eq!(
        idx.iter().find(|e| e.hash == "h2").unwrap().file_mtime,
        "2020-01-01T00:00:00Z"
    );

    // Not in the manifest until derivatives exist.
    assert!(store.manifest().await.unwrap().photos.is_empty());
    let q = store.status().await.unwrap();
    assert_eq!(q.derivative_queue, 2);
    let pending = store
        .pending_derivatives("2030-01-01T00:00:00Z", 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 2);

    // Failure backs off; success clears it and bumps generation.
    assert_eq!(
        store.record_derivative_failure("h2", "boom").await.unwrap(),
        1
    );
    assert_eq!(
        store.record_derivative_failure("h2", "boom").await.unwrap(),
        2
    );
    let now = crate::now();
    assert_eq!(store.pending_derivatives(&now, 10).await.unwrap().len(), 1);
    assert_eq!(store.status().await.unwrap().derivative_failures, 1);
    let g0 = store.generation().await.unwrap();
    store.mark_derivatives_ready("h1", 0).await.unwrap();
    assert_eq!(store.generation().await.unwrap(), g0 + 1);
    store.mark_derivatives_ready("h2", 0).await.unwrap();
    assert_eq!(store.status().await.unwrap().derivative_failures, 0);

    let m = store.manifest().await.unwrap();
    assert_eq!(m.photos.len(), 2);
    let h1 = m.photos.iter().find(|p| p.hash == "h1").unwrap();
    assert_eq!(h1.effective_date, "2024-05-01T10:00:00Z");
    assert_eq!(h1.date_source, "exif");
    assert_eq!(
        m.photos
            .iter()
            .find(|p| p.hash == "h2")
            .unwrap()
            .effective_date,
        "2020-01-01T00:00:00Z"
    );
    // Ordered by effective date.
    assert_eq!(m.photos[0].hash, "h2");

    // Shared-state mutations bump generation.
    let g = store.generation().await.unwrap();
    assert_eq!(store.toggle_favorite("h1").await.unwrap(), Some(true));
    assert_eq!(store.toggle_favorite("nope").await.unwrap(), None);
    assert!(store.add_photo_tag("h1", "Dogs").await.unwrap());
    assert!(store.add_photo_tag("h1", "dogs").await.unwrap()); // case-insensitive, idempotent
    assert!(!store.add_photo_tag("nope", "dogs").await.unwrap());
    assert_eq!(store.create_tag("DOGS").await.unwrap(), "Dogs");
    assert_eq!(store.create_tag("cats").await.unwrap(), "cats");
    assert!(
        store
            .set_date_override("h2", Some("1987-06-14T00:00:00Z"))
            .await
            .unwrap()
    );
    assert!(store.generation().await.unwrap() > g);
    let m = store.manifest().await.unwrap();
    let h1 = m.photos.iter().find(|p| p.hash == "h1").unwrap();
    assert!(h1.favorite);
    assert_eq!(h1.tags, vec!["Dogs"]);
    let h2 = m.photos.iter().find(|p| p.hash == "h2").unwrap();
    assert_eq!(
        (h2.effective_date.as_str(), h2.date_source.as_str()),
        ("1987-06-14T00:00:00Z", "override")
    );
    let counts: Vec<_> = m.tags.iter().map(|t| (t.name.as_str(), t.count)).collect();
    assert_eq!(counts, vec![("cats", 0), ("Dogs", 1)]);
    assert_eq!(store.list_tags().await.unwrap().len(), 2);

    // Curation round trip: export, wipe the photos, re-index, import.
    let export = store.curation_export().await.unwrap();
    assert_eq!(export.photos.len(), 2);
    assert_eq!(
        export.photos["h2"].date_override.as_deref(),
        Some("1987-06-14T00:00:00Z")
    );
    let json = serde_json::to_string(&export).unwrap();
    let export: Curation = serde_json::from_str(&json).unwrap();
    store.delete_photo("h1").await.unwrap();
    store.delete_photo("h2").await.unwrap();
    assert!(
        store
            .insert_photo(&photo("h1", "moved/a.jpg", Some("2024-05-01T10:00:00Z")))
            .await
            .unwrap()
    );
    assert!(
        store
            .insert_photo(&photo("h2", "moved/b.jpg", None))
            .await
            .unwrap()
    );
    let mut with_missing = export.clone();
    with_missing
        .photos
        .insert("gone".into(), Default::default());
    let stats = store.curation_import(&with_missing).await.unwrap();
    assert_eq!((stats.applied, stats.skipped_missing), (2, 1));
    let again = store.curation_export().await.unwrap();
    assert_eq!(again.photos, export.photos);
    assert_eq!(again.tags, export.tags);
    // Import is a merge: importing an empty export changes nothing.
    store
        .curation_import(&Curation {
            version: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(store.curation_export().await.unwrap().photos, export.photos);
    assert!(store.set_date_override("h2", None).await.unwrap());
    assert_eq!(
        store
            .curation_export()
            .await
            .unwrap()
            .photos
            .get("h2")
            .and_then(|p| p.date_override.clone()),
        None
    );

    // Delete requests hide a photo immediately; duplicates coalesce.
    store.mark_derivatives_ready("h1", 0).await.unwrap();
    store.mark_derivatives_ready("h2", 0).await.unwrap();
    assert!(matches!(
        store
            .enqueue_request(RequestKind::Delete, Some("nope"))
            .await,
        Err(Error::Invalid(_))
    ));
    let id = store
        .enqueue_request(RequestKind::Delete, Some("h1"))
        .await
        .unwrap();
    assert_eq!(
        store
            .enqueue_request(RequestKind::Delete, Some("h1"))
            .await
            .unwrap(),
        id
    );
    assert!(
        store
            .manifest()
            .await
            .unwrap()
            .photos
            .iter()
            .all(|p| p.hash != "h1")
    );
    let export_id = store
        .enqueue_request(RequestKind::Export, None)
        .await
        .unwrap();
    let claimed = store.claim_requests(10).await.unwrap();
    assert_eq!(claimed.len(), 2);
    assert!(claimed.iter().all(|r| r.state == "running"));
    assert!(store.claim_requests(10).await.unwrap().is_empty());
    store.reset_running_requests().await.unwrap();
    assert_eq!(store.claim_requests(10).await.unwrap().len(), 2);
    store.delete_photo("h1").await.unwrap();
    store.finish_request(id, true, None).await.unwrap();
    store
        .finish_request(export_id, true, Some("export.json"))
        .await
        .unwrap();
    let r = store.get_request(export_id).await.unwrap().unwrap();
    assert_eq!(
        (r.state.as_str(), r.result.as_deref()),
        ("done", Some("export.json"))
    );
    // A finished delete no longer coalesces.
    assert!(
        store
            .insert_photo(&photo("h1", "again/a.jpg", None))
            .await
            .unwrap()
    );
    assert_ne!(
        store
            .enqueue_request(RequestKind::Delete, Some("h1"))
            .await
            .unwrap(),
        id
    );

    store.set_meta("scan_total", "42").await.unwrap();
    assert_eq!(
        store.get_meta("scan_total").await.unwrap().as_deref(),
        Some("42")
    );
    assert_eq!(store.status().await.unwrap().scan_total, 42);
    reset(store).await;
}

/// Concurrent writers (the management UI tags a selection with several requests
/// in flight) must all succeed rather than fail with a lock error.
async fn concurrent_tagging(store: std::sync::Arc<dyn Store>) {
    store.migrate().await.unwrap();
    let hashes: Vec<String> = (0..12).map(|i| format!("c{i:02}")).collect();
    for h in &hashes {
        store.delete_photo(h).await.unwrap();
        store
            .insert_photo(&photo(h, &format!("conc/{h}.jpg"), None))
            .await
            .unwrap();
    }
    let mut tasks = Vec::new();
    for h in hashes.clone() {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            // Every task creates the same new tag, then toggles a favourite.
            store.add_photo_tag(&h, "Concurrent").await?;
            store.toggle_favorite(&h).await?;
            store.create_tag("concurrent").await?;
            Ok::<_, crate::Error>(())
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    let tag = store
        .list_tags()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Concurrent")
        .unwrap();
    assert_eq!(tag.count, 12);
    for h in &hashes {
        store.delete_photo(h).await.unwrap();
    }
}

#[tokio::test]
async fn sqlite_concurrent_writers() {
    let dir = std::env::temp_dir().join(format!("pf-store-conc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = connect(&format!("sqlite://{}/t.db", dir.display()))
        .await
        .unwrap();
    concurrent_tagging(store).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_concurrent_writers() {
    let Some(url) = fresh_postgres().await else {
        return;
    };
    concurrent_tagging(connect(&url).await.unwrap()).await;
}

async fn rotation_flow(store: &dyn Store) {
    store.migrate().await.unwrap();
    reset(store).await;
    store
        .insert_photo(&photo("h1", "r/a.jpg", None))
        .await
        .unwrap(); // 4000 x 3000
    store.mark_derivatives_ready("h1", 0).await.unwrap();

    // Relative and composable; wraps; unknown photograph is None.
    assert_eq!(store.rotate_photo("h1", 90).await.unwrap(), Some(90));
    assert_eq!(store.rotate_photo("h1", 270).await.unwrap(), Some(0));
    assert_eq!(store.rotate_photo("h1", -90).await.unwrap(), Some(270));
    assert_eq!(store.rotate_photo("h1", 180).await.unwrap(), Some(90));
    assert_eq!(store.rotate_photo("nope", 90).await.unwrap(), None);

    // Until the indexer catches up the manifest keeps serving the old
    // derivatives: old dimensions, old media rotation, new desired rotation.
    let p = &store.manifest().await.unwrap().photos[0];
    assert_eq!(
        (p.rotation, p.media_rotation, p.w, p.h),
        (90, 0, 4000, 3000)
    );
    let pending = store.pending_derivatives(&crate::now(), 10).await.unwrap();
    assert_eq!((pending.len(), pending[0].rotation), (1, 90));
    assert_eq!(store.status().await.unwrap().derivative_queue, 1);

    // Regenerating for a stale rotation leaves it pending (rotated again mid-job).
    store.rotate_photo("h1", 90).await.unwrap(); // now 180
    let g = store.generation().await.unwrap();
    store.mark_derivatives_ready("h1", 90).await.unwrap();
    assert!(store.generation().await.unwrap() > g);
    assert_eq!(
        store
            .pending_derivatives(&crate::now(), 10)
            .await
            .unwrap()
            .len(),
        1
    );

    // Caught up: dimensions swap for a quarter turn, and nothing is pending.
    store.rotate_photo("h1", 270).await.unwrap(); // back to 90
    store.mark_derivatives_ready("h1", 90).await.unwrap();
    let p = &store.manifest().await.unwrap().photos[0];
    assert_eq!(
        (p.rotation, p.media_rotation, p.w, p.h),
        (90, 90, 3000, 4000)
    );
    assert!(
        store
            .pending_derivatives(&crate::now(), 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.status().await.unwrap().derivative_queue, 0);

    // Rotation is authoritative curation: it survives export and a rebuild.
    let export = store.curation_export().await.unwrap();
    assert_eq!(export.photos["h1"].rotation, 90);
    let json = serde_json::to_string(&export).unwrap();
    assert!(json.contains("\"rotation\":90"));
    let unrotated = serde_json::to_string(&crate::CuratedPhoto::default()).unwrap();
    assert!(
        !unrotated.contains("rotation"),
        "zero is omitted: {unrotated}"
    );
    store.delete_photo("h1").await.unwrap();
    store
        .insert_photo(&photo("h1", "r/a.jpg", None))
        .await
        .unwrap();
    store
        .curation_import(&serde_json::from_str(&json).unwrap())
        .await
        .unwrap();
    assert_eq!(
        store.curation_export().await.unwrap().photos["h1"].rotation,
        90
    );
    // Merge only: an export without rotation never clears it; bad values are refused.
    let mut none = export.clone();
    none.photos.get_mut("h1").unwrap().rotation = 0;
    store.curation_import(&none).await.unwrap();
    assert_eq!(
        store.curation_export().await.unwrap().photos["h1"].rotation,
        90
    );
    let mut bad = export;
    bad.photos.get_mut("h1").unwrap().rotation = 45;
    assert!(matches!(
        store.curation_import(&bad).await,
        Err(Error::Invalid(_))
    ));
    reset(store).await;
}

#[tokio::test]
async fn sqlite_rotation() {
    let dir = std::env::temp_dir().join(format!("pf-store-rot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = connect(&format!("sqlite://{}/t.db", dir.display()))
        .await
        .unwrap();
    rotation_flow(store.as_ref()).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_rotation() {
    let Some(url) = fresh_postgres().await else {
        return;
    };
    rotation_flow(connect(&url).await.unwrap().as_ref()).await;
}
