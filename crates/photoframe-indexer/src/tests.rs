//! Integration tests over a temporary volume and a real SQLite file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use photoframe_store::RequestKind;

use crate::ctx::Ctx;
use crate::scan::Scanner;
use crate::{derivatives, promote, requests};

struct Env {
    dir: tempfile::TempDir,
    ctx: Ctx,
}

impl Env {
    async fn new() -> Self {
        Self::with(&[]).await
    }

    async fn with(extra: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vol");
        for d in [
            "incoming",
            "library",
            "derivatives",
            "quarantine",
            "exports",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let db = dir.path().join("t.db");
        let mut env: HashMap<String, String> = HashMap::new();
        env.insert("DATABASE_URL".into(), format!("sqlite://{}", db.display()));
        env.insert("LIBRARY_ROOT".into(), root.display().to_string());
        env.insert("INGEST_QUIET_SECS".into(), "0".into());
        env.insert("DERIVATIVE_WORKERS".into(), "2".into());
        for (k, v) in extra {
            env.insert(k.to_string(), v.to_string());
        }
        let config = crate::config::Config::from_map(&env).unwrap();
        let store = photoframe_store::connect(&config.database_url)
            .await
            .unwrap();
        store.migrate().await.unwrap();
        Self {
            ctx: Ctx::new(store, config),
            dir,
        }
    }

    fn vol(&self) -> PathBuf {
        self.dir.path().join("vol")
    }

    fn lib(&self, rel: &str) -> PathBuf {
        self.vol().join("library").join(rel)
    }

    fn put(&self, area: &str, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.vol().join(area).join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }

    async fn scan(&self, scanner: &mut Scanner) -> crate::scan::ScanStats {
        scanner.reconcile(&self.ctx).await.unwrap()
    }
}

/// A small JPEG whose content (and so hash) depends on `seed`.
fn jpeg(seed: u8) -> Vec<u8> {
    let (w, h) = (96u32, 64u32);
    let mut rgb = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgb.extend_from_slice(&[(x as u8).wrapping_add(seed), y as u8, seed.wrapping_mul(7)]);
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 85)
        .encode(&rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    out
}

fn hash_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn has_derivatives(env: &Env, hash: &str) -> bool {
    has_derivatives_rot(env, hash, 0)
}

fn has_derivatives_rot(env: &Env, hash: &str, rotation: i32) -> bool {
    imagepipe::VARIANTS.iter().all(|v| {
        env.ctx
            .layout
            .derivative_path(hash, v.name, rotation, "jpg")
            .unwrap()
            .is_file()
    })
}

fn files_under(p: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(d).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(p).unwrap().display().to_string());
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn new_moved_and_deleted_files() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(1);
    let hash = hash_of(&bytes);
    env.put("library", "2024/01/a.jpg", &bytes);

    // New file: indexed, hidden from the manifest until derivatives exist.
    assert_eq!(env.scan(&mut sc).await.added, 1);
    assert!(env.ctx.store.manifest().await.unwrap().photos.is_empty());
    let d = derivatives::run(&env.ctx).await.unwrap();
    assert_eq!((d.generated, d.failed), (1, 0));
    assert!(has_derivatives(&env, &hash));
    let m = env.ctx.store.manifest().await.unwrap();
    assert_eq!((m.photos.len(), m.photos[0].w, m.photos[0].h), (1, 96, 64));
    assert_eq!(m.photos[0].date_source, "mtime");

    // Steady state changes nothing.
    let s = env.scan(&mut sc).await;
    assert_eq!((s.added, s.moved, s.removed), (0, 0, 0));

    // Curate, then move: identity is content, so curation follows the file.
    env.ctx.store.toggle_favorite(&hash).await.unwrap();
    env.ctx.store.add_photo_tag(&hash, "dogs").await.unwrap();
    fs::create_dir_all(env.lib("2024/02")).unwrap();
    fs::rename(env.lib("2024/01/a.jpg"), env.lib("2024/02/renamed.jpg")).unwrap();
    let s = env.scan(&mut sc).await;
    assert_eq!((s.added, s.moved, s.removed), (0, 1, 0));
    let m = env.ctx.store.manifest().await.unwrap();
    assert_eq!(m.photos.len(), 1);
    assert!(m.photos[0].favorite);
    assert_eq!(m.photos[0].tags, vec!["dogs"]);
    assert!(has_derivatives(&env, &hash)); // not regenerated, not removed

    // Deleted: row and derivatives go.
    fs::remove_file(env.lib("2024/02/renamed.jpg")).unwrap();
    // A lone empty listing is not trusted the first time (see the safety test).
    env.scan(&mut sc).await;
    let s = env.scan(&mut sc).await;
    assert_eq!(s.removed, 1);
    assert!(env.ctx.store.manifest().await.unwrap().photos.is_empty());
    assert!(!has_derivatives(&env, &hash));
}

#[tokio::test]
async fn duplicate_content_is_one_photograph() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(2);
    env.put("library", "a/one.jpg", &bytes);
    env.put("library", "b/copy.jpg", &bytes);
    let s = env.scan(&mut sc).await;
    assert_eq!(s.added, 1);
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 1);
    // Removing the copy leaves the photograph alone.
    fs::remove_file(env.lib("b/copy.jpg")).unwrap();
    let s = env.scan(&mut sc).await;
    assert_eq!(s.removed, 0);
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 1);
}

#[tokio::test]
async fn non_images_are_ignored_and_not_rehashed() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    env.put("library", "notes.txt", b"hello");
    env.put("library", "fake.jpg", b"\xFF\xD8\xFF\xE0 not really");
    env.put("library", ".hidden.jpg", &jpeg(3));
    env.put("library", "half.jpg.part", &jpeg(3));
    let s = env.scan(&mut sc).await;
    assert_eq!((s.added, s.skipped), (0, 2));
    let s = env.scan(&mut sc).await;
    assert_eq!((s.added, s.skipped), (0, 2));
    assert!(env.ctx.store.list_index().await.unwrap().is_empty());
}

#[tokio::test]
async fn changed_content_at_same_path_is_a_new_photograph() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    env.put("library", "x.jpg", &jpeg(4));
    env.scan(&mut sc).await;
    let old = env.ctx.store.list_index().await.unwrap()[0].hash.clone();
    let p = env.put("library", "x.jpg", &jpeg(5));
    // A real replacement has a later mtime; size+mtime is the change detector.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let s = env.scan(&mut sc).await;
    assert_eq!((s.added, s.removed), (1, 1));
    let idx = env.ctx.store.list_index().await.unwrap();
    assert_eq!(idx.len(), 1);
    assert_ne!(idx[0].hash, old);
}

#[tokio::test]
async fn empty_database_rebuilds_without_regenerating_derivatives() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    for i in 0..3u8 {
        env.put("library", &format!("2024/{i}.jpg"), &jpeg(10 + i));
    }
    env.scan(&mut sc).await;
    assert_eq!(derivatives::run(&env.ctx).await.unwrap().generated, 3);

    // A brand new database over the same volume.
    let fresh = Env::with(&[]).await;
    let ctx = Ctx::new(fresh.ctx.store.clone(), {
        let mut c = (*fresh.ctx.config).clone();
        c.library_root = env.vol();
        c
    });
    let mut sc2 = Scanner::default();
    assert_eq!(sc2.reconcile(&ctx).await.unwrap().added, 3);
    let d = derivatives::run(&ctx).await.unwrap();
    assert_eq!((d.generated, d.already_present), (0, 3));
    assert_eq!(ctx.store.manifest().await.unwrap().photos.len(), 3);
}

#[tokio::test]
async fn unreadable_or_empty_listing_does_not_wipe_the_index() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    env.put("library", "a.jpg", &jpeg(6));
    env.scan(&mut sc).await;

    // Volume "unmounted": library/ missing entirely. The scan errors out.
    fs::rename(env.vol().join("library"), env.vol().join("library.away")).unwrap();
    assert!(sc.reconcile(&env.ctx).await.is_err());
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 1);

    // Mounted but empty: only believed on the second consecutive scan.
    fs::create_dir(env.vol().join("library")).unwrap();
    assert_eq!(env.scan(&mut sc).await.removed, 0);
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 1);
    fs::remove_dir(env.vol().join("library")).unwrap();
    fs::rename(env.vol().join("library.away"), env.vol().join("library")).unwrap();
    assert_eq!(env.scan(&mut sc).await.removed, 0); // back, and strikes reset
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 1);
}

#[tokio::test]
async fn missing_source_backs_off_instead_of_looping() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    env.put("library", "a.jpg", &jpeg(7));
    env.scan(&mut sc).await;
    fs::remove_file(env.lib("a.jpg")).unwrap();
    let d = derivatives::run(&env.ctx).await.unwrap();
    assert_eq!(d.failed, 1);
    assert_eq!(env.ctx.store.status().await.unwrap().derivative_failures, 1);
    // Backing off: nothing eligible right now.
    let d = derivatives::run(&env.ctx).await.unwrap();
    assert_eq!((d.generated, d.failed), (0, 0));
}

#[tokio::test]
async fn promotion_files_by_date_dedupes_and_quarantines() {
    let env = Env::new().await;
    let good = jpeg(20);
    env.put("incoming", "IMG 001.JPG", &good);
    env.put("incoming", "nested/IMG 002.jpeg", &jpeg(21));
    env.put("incoming", "evil.jpg", b"<svg onload=alert(1)>");
    env.put("incoming", "half.jpg.part", &good);

    let p = promote::promote(&env.ctx).await.unwrap();
    assert_eq!((p.promoted, p.quarantined, p.duplicates), (2, 1, 0));

    let lib = files_under(&env.vol().join("library"));
    assert_eq!(lib.len(), 2, "{lib:?}");
    assert!(
        lib.iter().all(|f| f.ends_with(".jpg")),
        "extension follows the detected format: {lib:?}"
    );
    let year_month = regex_ym(&lib[0]);
    assert!(year_month, "{lib:?}");
    assert_eq!(env.ctx.store.list_index().await.unwrap().len(), 2);
    assert!(env.vol().join("incoming/half.jpg.part").exists()); // partial upload untouched

    let q = files_under(&env.vol().join("quarantine"));
    assert_eq!(q.len(), 2, "{q:?}");
    let reason = fs::read_to_string(
        env.vol()
            .join("quarantine")
            .join(q.iter().find(|f| f.ends_with(".reason.txt")).unwrap()),
    )
    .unwrap();
    assert!(reason.contains("unsupported"), "{reason}");

    // The same bytes again: a duplicate, removed from incoming.
    env.put("incoming", "again.jpg", &good);
    let p = promote::promote(&env.ctx).await.unwrap();
    assert_eq!((p.promoted, p.duplicates), (0, 1));
    assert!(!env.vol().join("incoming/again.jpg").exists());

    // Filename collision in the same month resolves with -1.
    env.put("incoming", "IMG 001.jpg", &jpeg(22));
    promote::promote(&env.ctx).await.unwrap();
    let lib = files_under(&env.vol().join("library"));
    assert!(lib.iter().any(|f| f.ends_with("IMG 001-1.jpg")), "{lib:?}");

    // What was promoted is already indexed, so the next walk adds nothing.
    let mut sc = Scanner::default();
    assert_eq!(sc.reconcile(&env.ctx).await.unwrap().added, 0);
}

fn regex_ym(p: &str) -> bool {
    let parts: Vec<_> = p.split('/').collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[0].bytes().all(|b| b.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].bytes().all(|b| b.is_ascii_digit())
}

#[tokio::test]
async fn promotion_respects_quiet_period_and_size_limit() {
    let env = Env::with(&[("INGEST_QUIET_SECS", "3600"), ("MAX_UPLOAD_BYTES", "100")]).await;
    env.put("incoming", "fresh.jpg", &jpeg(30));
    let p = promote::promote(&env.ctx).await.unwrap();
    assert_eq!((p.promoted, p.too_new), (0, 1));

    let env = Env::with(&[("MAX_UPLOAD_BYTES", "100")]).await;
    env.put("incoming", "big.jpg", &jpeg(31));
    let p = promote::promote(&env.ctx).await.unwrap();
    assert_eq!((p.promoted, p.quarantined), (0, 1));
    let q = files_under(&env.vol().join("quarantine"));
    assert!(
        q.iter().any(|f| f == "big.jpg") && q.iter().any(|f| f == "big.jpg.reason.txt"),
        "{q:?}"
    );
}

#[tokio::test]
async fn delete_and_export_requests() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(40);
    let hash = hash_of(&bytes);
    env.put("library", "2024/del.jpg", &bytes);
    env.scan(&mut sc).await;
    derivatives::run(&env.ctx).await.unwrap();
    env.ctx.store.toggle_favorite(&hash).await.unwrap();

    let export_id = env
        .ctx
        .store
        .enqueue_request(RequestKind::Export, None)
        .await
        .unwrap();
    let del_id = env
        .ctx
        .store
        .enqueue_request(RequestKind::Delete, Some(&hash))
        .await
        .unwrap();
    // Hidden from the manifest as soon as the request exists.
    assert!(env.ctx.store.manifest().await.unwrap().photos.is_empty());
    assert_eq!(requests::process(&env.ctx).await.unwrap(), 2);

    assert!(!env.lib("2024/del.jpg").exists());
    assert!(!has_derivatives(&env, &hash));
    assert!(env.ctx.store.list_index().await.unwrap().is_empty());
    assert_eq!(
        env.ctx
            .store
            .get_request(del_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        "done"
    );

    let r = env.ctx.store.get_request(export_id).await.unwrap().unwrap();
    assert_eq!(r.state, "done");
    let name = r.result.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(env.vol().join("exports").join(&name)).unwrap()).unwrap();
    // The export ran first, before the delete, so it still holds the favourite.
    assert_eq!(json["photos"][&hash]["favorite"], true);
    assert_eq!(json["version"], 1);
}

#[tokio::test]
async fn cli_export_import_round_trip() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(50);
    let hash = hash_of(&bytes);
    env.put("library", "a.jpg", &bytes);
    env.scan(&mut sc).await;
    env.ctx.store.toggle_favorite(&hash).await.unwrap();
    env.ctx
        .store
        .add_photo_tag(&hash, "christmas")
        .await
        .unwrap();
    env.ctx
        .store
        .set_date_override(&hash, Some("1987-06-14T00:00:00Z"))
        .await
        .unwrap();
    let out = env.dir.path().join("c.json");
    requests::cli_export(&env.ctx, &out).await.unwrap();

    // Recover into an empty database over the same library: rebuild, then import.
    let fresh = Env::with(&[]).await;
    let ctx = Ctx::new(fresh.ctx.store.clone(), {
        let mut c = (*fresh.ctx.config).clone();
        c.library_root = env.vol();
        c
    });
    Scanner::default().reconcile(&ctx).await.unwrap();
    let stats = requests::cli_import(&ctx, &out).await.unwrap();
    assert_eq!((stats.applied, stats.skipped_missing), (1, 0));
    let a = env.ctx.store.curation_export().await.unwrap();
    let b = ctx.store.curation_export().await.unwrap();
    assert_eq!((a.tags, a.photos), (b.tags, b.photos));
}

#[tokio::test]
async fn rotation_regenerates_derivatives_and_prunes_the_old_ones() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(60); // 96 x 64
    let hash = hash_of(&bytes);
    env.put("library", "r.jpg", &bytes);
    env.scan(&mut sc).await;
    derivatives::run(&env.ctx).await.unwrap();
    let p = &env.ctx.store.manifest().await.unwrap().photos[0];
    assert_eq!((p.w, p.h, p.media_rotation), (96, 64, 0));

    // Curate a quarter turn. The manifest keeps the old derivatives until the
    // indexer has made new ones.
    env.ctx.store.rotate_photo(&hash, 90).await.unwrap();
    let p = &env.ctx.store.manifest().await.unwrap().photos[0];
    assert_eq!((p.rotation, p.media_rotation, p.w, p.h), (90, 0, 96, 64));
    assert!(has_derivatives_rot(&env, &hash, 0));

    let d = derivatives::run(&env.ctx).await.unwrap();
    assert_eq!((d.generated, d.failed), (1, 0));
    assert!(has_derivatives_rot(&env, &hash, 90));
    assert!(
        !has_derivatives_rot(&env, &hash, 0),
        "old rotation's files are pruned"
    );
    let p = &env.ctx.store.manifest().await.unwrap().photos[0];
    assert_eq!((p.rotation, p.media_rotation, p.w, p.h), (90, 90, 64, 96));
    // The original is never touched.
    assert_eq!(hash_of(&fs::read(env.lib("r.jpg")).unwrap()), hash);

    // Nothing left to do, and a rebuild finds the rotated files already present.
    let d = derivatives::run(&env.ctx).await.unwrap();
    assert_eq!((d.generated, d.already_present), (0, 0));

    // Back to upright; a delete removes whichever rotation is current.
    env.ctx.store.rotate_photo(&hash, 270).await.unwrap();
    derivatives::run(&env.ctx).await.unwrap();
    assert!(has_derivatives_rot(&env, &hash, 0) && !has_derivatives_rot(&env, &hash, 90));
}

#[tokio::test]
async fn rotation_survives_curation_export_and_rebuild() {
    let env = Env::new().await;
    let mut sc = Scanner::default();
    let bytes = jpeg(61);
    let hash = hash_of(&bytes);
    env.put("library", "r.jpg", &bytes);
    env.scan(&mut sc).await;
    env.ctx.store.rotate_photo(&hash, 180).await.unwrap();
    let out = env.dir.path().join("c.json");
    requests::cli_export(&env.ctx, &out).await.unwrap();

    let fresh = Env::with(&[]).await;
    let ctx = Ctx::new(fresh.ctx.store.clone(), {
        let mut c = (*fresh.ctx.config).clone();
        c.library_root = env.vol();
        c
    });
    Scanner::default().reconcile(&ctx).await.unwrap();
    requests::cli_import(&ctx, &out).await.unwrap();
    derivatives::run(&ctx).await.unwrap();
    let p = &ctx.store.manifest().await.unwrap().photos[0];
    assert_eq!((p.rotation, p.media_rotation), (180, 180));
}
