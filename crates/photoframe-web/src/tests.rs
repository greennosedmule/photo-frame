//! Route-level tests: the real router over in-memory SQLite and a temp volume.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use base64::Engine;
use http_body_util::BodyExt;
use photoframe_store::{NewPhoto, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::app::{AppState, router};
use crate::auth::BasicAuth;
use crate::config::Config;

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

struct T {
    app: Router,
    store: Arc<dyn Store>,
    dir: tempfile::TempDir,
}

async fn setup_with(max_upload: &str) -> T {
    let dir = tempfile::tempdir().unwrap();
    let store = photoframe_store::connect("sqlite::memory:").await.unwrap();
    store.migrate().await.unwrap();
    let env = [
        ("DATABASE_URL", "sqlite::memory:"),
        ("LIBRARY_ROOT", dir.path().to_str().unwrap()),
        ("ADMIN_PASSWORD", "pw"),
        ("MAX_UPLOAD_BYTES", max_upload),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let config = Config::from_map(&env).unwrap();
    let app = router(AppState {
        store: store.clone(),
        auth: Arc::new(BasicAuth::new("admin".into(), "pw".into())),
        layout: photoframe_store::Layout::new(dir.path()),
        config: Arc::new(config),
    });
    T { app, store, dir }
}

async fn setup() -> T {
    setup_with("1048576").await
}

fn basic() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("admin:pw")
    )
}

impl T {
    async fn send(&self, req: Request<Body>) -> Response<Body> {
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn get(&self, uri: &str) -> Response<Body> {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    async fn json(&self, method: &str, uri: &str, body: Value, auth: bool) -> Response<Body> {
        let mut r = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if auth {
            r = r.header(header::AUTHORIZATION, basic());
        }
        self.send(r.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn authed(&self, method: &str, uri: &str) -> Response<Body> {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::AUTHORIZATION, basic())
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// An indexed photograph with derivatives on disk.
    async fn photo(&self, hash: &str, taken: Option<&str>) {
        let p = NewPhoto {
            hash: hash.into(),
            rel_path: format!("{}.jpg", &hash[..4]),
            media_type: "image".into(),
            mime: "image/jpeg".into(),
            byte_size: 10,
            width: 400,
            height: 300,
            orientation: 1,
            taken_at: taken.map(Into::into),
            file_mtime: "2020-01-01T00:00:00Z".into(),
            date_source: if taken.is_some() { "exif" } else { "mtime" }.into(),
        };
        self.store.insert_photo(&p).await.unwrap();
        self.store.mark_derivatives_ready(hash, 0).await.unwrap();
        let layout = photoframe_store::Layout::new(self.dir.path());
        let path = layout.derivative_path(hash, "thumb", 0, "jpg").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"0123456789abcdef").unwrap();
    }
}

async fn body_bytes(r: Response<Body>) -> Vec<u8> {
    r.into_body().collect().await.unwrap().to_bytes().to_vec()
}

async fn body_json(r: Response<Body>) -> Value {
    serde_json::from_slice(&body_bytes(r).await).unwrap()
}

// ---- status and auth -------------------------------------------------------

#[tokio::test]
async fn status_reports_counts_and_generation() {
    let t = setup().await;
    let v = body_json(t.get("/api/status").await).await;
    assert_eq!(v["generation"], 0);
    assert_eq!(v["indexing"], false);
    assert_eq!(v["photo_count"], 0);
    t.photo(H1, None).await;
    let v = body_json(t.get("/api/status").await).await;
    assert_eq!(v["photo_count"], 1);
    assert!(v["generation"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn managed_routes_require_auth() {
    let t = setup().await;
    t.photo(H1, None).await;
    for (m, uri) in [
        ("POST", "/api/scan"),
        ("GET", "/api/session"),
        ("POST", "/api/upload"),
        ("PATCH", &format!("/api/photos/{H1}")),
        ("DELETE", &format!("/api/photos/{H1}")),
        ("POST", "/api/export"),
        ("GET", "/api/requests/1"),
        ("GET", "/api/exports/curation-1.json"),
    ] {
        let res = t
            .send(
                Request::builder()
                    .method(m)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{m} {uri}");
        assert!(res.headers().get(header::WWW_AUTHENTICATE).is_none());
        assert_eq!(
            res.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
    }
    // Favourite, tagging and reads are open: the frame is a kiosk.
    assert_eq!(
        t.json(
            "POST",
            &format!("/api/photos/{H1}/favorite"),
            json!({}),
            false
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn session_check_accepts_only_valid_credentials() {
    let t = setup().await;
    assert_eq!(
        t.authed("GET", "/api/session").await.status(),
        StatusCode::NO_CONTENT
    );
    let bad = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("admin:nope")
    );
    let res = t
        .send(
            Request::get("/api/session")
                .header(header::AUTHORIZATION, bad)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn scan_with_auth_sets_the_flag() {
    let t = setup().await;
    assert_eq!(
        t.authed("POST", "/api/scan").await.status(),
        StatusCode::ACCEPTED
    );
    assert!(t.store.take_scan_request().await.unwrap());
}

// ---- manifest ---------------------------------------------------------------

#[tokio::test]
async fn manifest_shape_etag_and_304() {
    let t = setup().await;
    t.photo(H1, Some("2026-07-04T18:22:41Z")).await;
    t.photo(H2, None).await;
    t.store.add_photo_tag(H1, "holidays").await.unwrap();
    // A photograph without derivatives never appears.
    t.store
        .insert_photo(&NewPhoto {
            hash: "3".repeat(64),
            rel_path: "x.jpg".into(),
            media_type: "image".into(),
            mime: "image/jpeg".into(),
            byte_size: 1,
            width: 1,
            height: 1,
            orientation: 1,
            taken_at: None,
            file_mtime: "2020-01-01T00:00:00Z".into(),
            date_source: "mtime".into(),
        })
        .await
        .unwrap();

    let res = t.get("/api/manifest").await;
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers()[header::ETAG].to_str().unwrap().to_string();
    let m = body_json(res).await;
    assert_eq!(m["indexing"], false);
    let photos = m["photos"].as_array().unwrap();
    assert_eq!(photos.len(), 2);
    let p = photos.iter().find(|p| p["hash"] == H1).unwrap();
    assert_eq!(p["w"], 400);
    assert_eq!(p["h"], 300);
    assert_eq!(p["effective_date"], "2026-07-04T18:22:41Z");
    assert_eq!(p["date_source"], "exif");
    assert_eq!(p["favorite"], false);
    assert_eq!(p["tags"], json!(["holidays"]));
    assert_eq!(m["tags"], json!([{"name": "holidays", "count": 1}]));

    // Unchanged generation: 304 with no body.
    let res = t
        .send(
            Request::get("/api/manifest")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(body_bytes(res).await.is_empty());

    // Any shared-state change moves the ETag.
    t.store.toggle_favorite(H1).await.unwrap();
    let res = t
        .send(
            Request::get("/api/manifest")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_ne!(res.headers()[header::ETAG].to_str().unwrap(), etag);
}

// ---- favourites and tags ----------------------------------------------------

#[tokio::test]
async fn favorite_toggles_and_tags_work() {
    let t = setup().await;
    t.photo(H1, None).await;
    let uri = format!("/api/photos/{H1}/favorite");
    assert_eq!(
        body_json(t.json("POST", &uri, json!({}), false).await).await["favorite"],
        true
    );
    assert_eq!(
        body_json(t.json("POST", &uri, json!({}), false).await).await["favorite"],
        false
    );
    assert_eq!(
        t.json(
            "POST",
            &format!("/api/photos/{H2}/favorite"),
            json!({}),
            false
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.json("POST", "/api/photos/nothex/favorite", json!({}), false)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    let res = t
        .json("POST", "/api/tags", json!({"name": "  Dogs "}), false)
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(body_json(res).await["name"], "Dogs");
    // Case-insensitive: returns the canonical name.
    assert_eq!(
        body_json(
            t.json("POST", "/api/tags", json!({"name": "dogs"}), false)
                .await
        )
        .await["name"],
        "Dogs"
    );
    for bad in ["", "   ", &"x".repeat(65), "a\nb"] {
        assert_eq!(
            t.json("POST", "/api/tags", json!({"name": bad}), false)
                .await
                .status(),
            StatusCode::BAD_REQUEST,
            "{bad:?}"
        );
    }

    let tags_uri = format!("/api/photos/{H1}/tags");
    assert_eq!(
        t.json("POST", &tags_uri, json!({"add": "cats"}), false)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let m = body_json(t.get("/api/manifest").await).await;
    assert_eq!(m["photos"][0]["tags"], json!(["cats"]));
    assert_eq!(
        t.json("POST", &tags_uri, json!({"remove": "CATS"}), false)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body_json(t.get("/api/manifest").await).await["photos"][0]["tags"],
        json!([])
    );
    assert_eq!(
        t.json("POST", &tags_uri, json!({"add": "a", "remove": "b"}), false)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.json("POST", &tags_uri, json!({}), false).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.json(
            "POST",
            &format!("/api/photos/{H2}/tags"),
            json!({"add": "x"}),
            false
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let tags = body_json(t.get("/api/tags").await).await;
    assert_eq!(
        tags,
        json!([{"name": "cats", "count": 0}, {"name": "Dogs", "count": 0}])
    );

    // Malformed bodies are problem details, not plain text.
    let res = t
        .send(
            Request::post("/api/tags")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{nope"))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

// ---- curation ---------------------------------------------------------------

#[tokio::test]
async fn patch_delete_and_export_are_requests_or_db_writes() {
    let t = setup().await;
    t.photo(H1, Some("2026-07-04T18:22:41Z")).await;
    let uri = format!("/api/photos/{H1}");

    let res = t
        .json(
            "PATCH",
            &uri,
            json!({"date_override": "1987-06-14T02:00:00+02:00"}),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let m = body_json(t.get("/api/manifest").await).await;
    assert_eq!(m["photos"][0]["effective_date"], "1987-06-14T00:00:00Z");
    assert_eq!(m["photos"][0]["date_source"], "override");
    assert_eq!(
        t.json("PATCH", &uri, json!({"date_override": null}), true)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let m = body_json(t.get("/api/manifest").await).await;
    assert_eq!(m["photos"][0]["effective_date"], "2026-07-04T18:22:41Z");
    assert_eq!(m["photos"][0]["date_source"], "exif");
    assert_eq!(
        t.json(
            "PATCH",
            &uri,
            json!({"date_override": "last tuesday"}),
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.json("PATCH", &uri, json!({}), true).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.json(
            "PATCH",
            &format!("/api/photos/{H2}"),
            json!({"date_override": null}),
            true
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    // Delete only records a request, and hides the photograph at once.
    let res = t.authed("DELETE", &uri).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let id = body_json(res).await["request"].as_i64().unwrap();
    assert!(
        body_json(t.get("/api/manifest").await).await["photos"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = body_json(t.authed("GET", &format!("/api/requests/{id}")).await).await;
    assert_eq!(
        (r["kind"].as_str(), r["state"].as_str()),
        (Some("delete"), Some("pending"))
    );
    assert_eq!(
        t.authed("DELETE", &format!("/api/photos/{H2}"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // The file itself was not touched by the web tier.
    assert!(
        photoframe_store::Layout::new(t.dir.path())
            .derivative_path(H1, "thumb", 0, "jpg")
            .unwrap()
            .exists()
    );

    let res = t.authed("POST", "/api/export").await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert!(body_json(res).await["request"].as_i64().is_some());
    assert_eq!(
        t.authed("GET", "/api/requests/9999").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn export_download_serves_only_export_files() {
    let t = setup().await;
    let exports = t.dir.path().join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    std::fs::write(
        exports.join("curation-20260920T140211Z.json"),
        b"{\"version\":1}",
    )
    .unwrap();
    std::fs::write(t.dir.path().join("secret.json"), b"nope").unwrap();

    let res = t
        .authed("GET", "/api/exports/curation-20260920T140211Z.json")
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(body_bytes(res).await, b"{\"version\":1}");
    for bad in [
        "curation-1.json",
        "..%2Fsecret.json",
        "secret.json",
        "curation-.json",
        "curation-abc.json",
    ] {
        assert_eq!(
            t.authed("GET", &format!("/api/exports/{bad}"))
                .await
                .status(),
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
}

// ---- media ---------------------------------------------------------------------

#[tokio::test]
async fn media_is_immutable_ranged_and_leaks_nothing() {
    let t = setup().await;
    t.photo(H1, None).await;
    let res = t.get(&format!("/media/{H1}/thumb")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(body_bytes(res).await, b"0123456789abcdef");

    let res = t
        .send(
            Request::get(format!("/media/{H1}/thumb"))
                .header(header::RANGE, "bytes=4-7")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes 4-7/16");
    assert_eq!(body_bytes(res).await, b"4567");

    for uri in [
        format!("/media/{H1}/display"),      // not generated
        format!("/media/{H1}/original"),     // not a variant
        format!("/media/{H2}/thumb"),        // unknown photograph
        "/media/..%2F..%2Fetc/thumb".into(), // traversal
        format!("/media/{}/thumb", "AB".repeat(32)),
    ] {
        let res = t.get(&uri).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        let body = String::from_utf8(body_bytes(res).await).unwrap();
        assert!(
            !body.contains(t.dir.path().to_str().unwrap()),
            "{uri} leaked a path: {body}"
        );
    }
}

// ---- upload --------------------------------------------------------------------

const BOUNDARY: &str = "XBOUNDARYX";

fn multipart(parts: &[(&str, &[u8])]) -> Request<Body> {
    let mut body = Vec::new();
    for (name, bytes) in parts {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    Request::post("/api/upload")
        .header(header::AUTHORIZATION, basic())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap()
}

fn jpeg_bytes(extra: usize) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
    v.extend(std::iter::repeat_n(7u8, 32 + extra));
    v
}

/// Every file under `incoming/`, as `<dir>/<name>` relative paths.
fn incoming(t: &T) -> Vec<String> {
    let root = t.dir.path().join("incoming");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path().strip_prefix(&root).unwrap().display().to_string());
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn upload_streams_to_incoming_and_requests_a_scan() {
    let t = setup().await;
    let res = t
        .send(multipart(&[
            ("../../My Photo.jpg", &jpeg_bytes(5000)),
            ("b.jpg", &jpeg_bytes(1)),
        ]))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(res).await["received"], 2);
    let files = incoming(&t);
    assert_eq!(files.len(), 2, "{files:?}");
    // One directory per upload; the client's directory components are dropped
    // but the file name survives.
    assert!(
        files
            .iter()
            .all(|f| !f.ends_with(".part") && !f.contains("..") && f.matches('/').count() == 1),
        "{files:?}"
    );
    assert!(
        files.iter().any(|f| f.ends_with("/My Photo.jpg")),
        "{files:?}"
    );
    assert!(files.iter().any(|f| f.ends_with("/b.jpg")), "{files:?}");
    assert!(t.store.take_scan_request().await.unwrap());
}

#[tokio::test]
async fn upload_rejects_wrong_content_oversize_and_empty() {
    let t = setup_with("1000").await;
    // Magic bytes decide, not the file name.
    let res = t
        .send(multipart(&[(
            "evil.jpg",
            b"<html><script>alert(1)</script></html>",
        )]))
        .await;
    assert_eq!(res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // Over MAX_UPLOAD_BYTES while streaming: rejected and nothing left behind.
    let res = t.send(multipart(&[("big.jpg", &jpeg_bytes(5000))])).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(incoming(&t).is_empty(), "{:?}", incoming(&t));
    // No file at all.
    let res = t.send(multipart(&[])).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(!t.store.take_scan_request().await.unwrap());
}

// ---- rotation ------------------------------------------------------------------

#[tokio::test]
async fn rotate_is_relative_validated_and_wakes_the_indexer() {
    let t = setup().await;
    t.photo(H1, None).await;
    let uri = format!("/api/photos/{H1}");
    let rotate = |n: Value| json!({ "rotate": n });

    // Requires auth like every curation edit.
    assert_eq!(
        t.json("PATCH", &uri, rotate(json!(90)), false)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        t.json("PATCH", &uri, rotate(json!(90)), true)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        t.store.take_scan_request().await.unwrap(),
        "rotation nudges the indexer"
    );
    assert_eq!(
        t.json("PATCH", &uri, rotate(json!(90)), true)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.json("PATCH", &uri, rotate(json!(-270)), true)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let m = body_json(t.get("/api/manifest").await).await;
    let p = &m["photos"][0];
    // 90 + 90 - 270 = -90 = 270. Derivatives are not regenerated yet (no indexer here).
    assert_eq!(
        (p["rotation"].as_i64(), p["media_rotation"].as_i64()),
        (Some(270), Some(0))
    );
    assert_eq!((p["w"].as_i64(), p["h"].as_i64()), (Some(400), Some(300)));

    for bad in [
        json!(0),
        json!(45),
        json!(360),
        json!("90"),
        json!(1.5),
        json!(null),
        json!(90000000000i64),
    ] {
        assert_eq!(
            t.json("PATCH", &uri, rotate(bad.clone()), true)
                .await
                .status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(
        t.json("PATCH", &uri, json!({}), true).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.json(
            "PATCH",
            &format!("/api/photos/{H2}"),
            rotate(json!(90)),
            true
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    // Date and rotation may be sent together.
    let both = json!({ "rotate": 90, "date_override": "1990-01-01T00:00:00Z" });
    assert_eq!(
        t.json("PATCH", &uri, both, true).await.status(),
        StatusCode::NO_CONTENT
    );
    let m = body_json(t.get("/api/manifest").await).await;
    assert_eq!(m["photos"][0]["rotation"], 0);
    assert_eq!(m["photos"][0]["effective_date"], "1990-01-01T00:00:00Z");
}

#[tokio::test]
async fn media_rotation_selects_a_different_immutable_file() {
    let t = setup().await;
    t.photo(H1, None).await;
    let layout = photoframe_store::Layout::new(t.dir.path());
    let rotated = layout.derivative_path(H1, "thumb", 90, "jpg").unwrap();
    std::fs::write(&rotated, b"ROTATED-BYTES").unwrap();

    let plain = t.get(&format!("/media/{H1}/thumb")).await;
    assert_eq!(body_bytes(plain).await, b"0123456789abcdef");
    let res = t.get(&format!("/media/{H1}/thumb?r=90")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(body_bytes(res).await, b"ROTATED-BYTES");

    // Only real rotations, and only files that exist.
    for r in ["45", "-90", "360", "180", "abc"] {
        let status = t.get(&format!("/media/{H1}/thumb?r={r}")).await.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST,
            "r={r}: {status}"
        );
    }
}

#[tokio::test]
async fn cidr_auth_grants_management_by_source_address() {
    use axum::extract::ConnectInfo;
    use std::net::SocketAddr;

    let t = setup().await;
    let auth = crate::auth::CidrAuth::new(
        vec!["192.168.1.0/24".parse().unwrap()],
        vec!["10.0.0.0/8".parse().unwrap()],
    );
    let app = router(AppState {
        store: t.store.clone(),
        auth: Arc::new(auth),
        layout: photoframe_store::Layout::new(t.dir.path()),
        config: Arc::new(
            Config::from_map(
                &[
                    ("DATABASE_URL", "sqlite::memory:"),
                    ("LIBRARY_ROOT", "."),
                    ("ADMIN_ALLOWED_CIDRS", "192.168.1.0/24"),
                ]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            )
            .unwrap(),
        ),
    });
    let status = |peer: &str, xff: Option<&str>| {
        let app = app.clone();
        let peer: SocketAddr = peer.parse().unwrap();
        let mut r = Request::get("/api/session");
        if let Some(x) = xff {
            r = r.header("x-forwarded-for", x);
        }
        let mut req = r.body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo(peer));
        async move { app.oneshot(req).await.unwrap().status() }
    };
    assert_eq!(
        status("192.168.1.9:5000", None).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status("172.16.0.1:5000", None).await,
        StatusCode::UNAUTHORIZED
    );
    // Spoofed header from an untrusted peer is ignored.
    assert_eq!(
        status("172.16.0.1:5000", Some("192.168.1.9")).await,
        StatusCode::UNAUTHORIZED
    );
    // Via the trusted ingress the forwarded address decides.
    assert_eq!(
        status("10.0.0.2:5000", Some("192.168.1.9")).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status("10.0.0.2:5000", Some("203.0.113.9")).await,
        StatusCode::UNAUTHORIZED
    );
}
