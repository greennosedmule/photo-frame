//! `/media/{hash}/{variant}`: derivative bytes, content-addressed.

use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::app::AppState;
use crate::problem::Problem;

#[derive(serde::Deserialize)]
pub struct MediaQuery {
    /// The manifest's `media_rotation`. Part of the URL so a rotated photograph
    /// has a new, still-immutable, address.
    r: Option<i32>,
}

const VARIANTS: [&str; 4] = ["display", "preview", "thumb", "blur"];

/// `ServeFile` gives Range support (the video seam), a content type from the
/// extension and 404 without a body, so no filesystem path can leak. The URL
/// names the content, so the response can be cached forever.
pub async fn serve(
    State(s): State<AppState>,
    Path((hash, variant)): Path<(String, String)>,
    Query(q): Query<MediaQuery>,
    req: Request,
) -> Result<Response, Problem> {
    if !VARIANTS.contains(&variant.as_str()) {
        return Err(Problem::not_found());
    }
    let path = s
        .layout
        .derivative_path(&hash, &variant, q.r.unwrap_or(0), "jpg")
        .ok_or_else(Problem::not_found)?;
    let res = ServeFile::new(path)
        .oneshot(req)
        .await
        .map_err(|_| Problem::internal())?;
    if res.status() == StatusCode::NOT_FOUND {
        return Err(Problem::not_found());
    }
    let mut res = res.map(axum::body::Body::new);
    if res.status().is_success() {
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            "public, max-age=31536000, immutable"
                .parse()
                .map_err(|_| Problem::internal())?,
        );
    }
    Ok(res)
}
