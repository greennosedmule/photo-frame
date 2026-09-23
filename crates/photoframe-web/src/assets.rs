//! Static client assets, embedded from `web/dist` at compile time.
//! Build the client first (`npm run build` in `web/`); if `dist` is absent the
//! binary still builds and serves a placeholder.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

// $CARGO_MANIFEST_DIR makes this absolute in debug builds too. Without it,
// rust-embed's dev-mode disk reads resolve "../../web/dist" relative to the
// process's CWD, not this crate's directory - wrong (and 404s as "client not
// built") for any debug run started from somewhere else, e.g. `web/` in e2e CI.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
struct Assets;

/// Serve an asset by path, falling back to `index.html` so client-side routes
/// such as `/manage` resolve. `/api` and `/media` never reach this handler.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    let (file, name) = match Assets::get(path) {
        Some(f) => (f, path),
        // A missing file with an extension is a real 404, not a client route.
        None if path.rsplit('/').next().is_some_and(|s| s.contains('.')) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        None => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => {
                tracing::warn!("client not built: index.html missing from embedded/on-disk assets");
                return (StatusCode::NOT_FOUND, "client not built").into_response();
            }
        },
    };

    let mime = mime_for(name);
    // Hashed build output is immutable; everything else (index.html, sw.js) must revalidate.
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(file.data.into_owned()))
        .expect("valid response")
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}
