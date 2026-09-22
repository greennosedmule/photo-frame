use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, patch, post};
use photoframe_store::{Layout, Store};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{self, AuthProvider};
use crate::config::Config;
use crate::problem::Problem;
use crate::{api, media, upload};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub auth: Arc<dyn AuthProvider>,
    pub config: Arc<Config>,
    pub layout: Layout,
}

/// A request can carry a handful of photographs; each is still capped at
/// `MAX_UPLOAD_BYTES` while streaming.
const MAX_FILES_PER_UPLOAD: u64 = 25;

pub fn router(state: AppState) -> Router {
    let upload_limit = state
        .config
        .max_upload_bytes
        .saturating_mul(MAX_FILES_PER_UPLOAD)
        .saturating_add(1024 * 1024);

    // Routes that need Basic auth. Everything else, including favourite and
    // tagging, is deliberately open because the frame is a kiosk.
    let managed = Router::new()
        .route("/api/scan", post(request_scan))
        // Lets the management login form check credentials without side effects.
        .route(
            "/api/session",
            get(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .route(
            "/api/upload",
            post(upload::upload).layer(DefaultBodyLimit::max(
                usize::try_from(upload_limit).unwrap_or(usize::MAX),
            )),
        )
        .route(
            "/api/photos/{hash}",
            patch(api::patch_photo).delete(api::delete_photo),
        )
        .route("/api/export", post(api::request_export))
        .route("/api/requests/{id}", get(api::get_request))
        .route("/api/exports/{name}", get(api::download_export))
        .layer(from_fn_with_state(state.auth.clone(), auth::require_auth));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/status", get(api::status))
        .route("/api/manifest", get(api::manifest))
        .route("/api/tags", get(api::tags).post(api::create_tag))
        .route("/api/photos/{hash}/favorite", post(api::toggle_favorite))
        .route("/api/photos/{hash}/tags", post(api::change_photo_tag))
        .route("/media/{hash}/{variant}", get(media::serve))
        .merge(managed)
        .fallback(crate::assets::serve)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Only records the request; the indexer starts a scan on its next tick.
async fn request_scan(
    axum::extract::State(s): axum::extract::State<AppState>,
) -> Result<impl axum::response::IntoResponse, Problem> {
    s.store.request_scan().await?;
    Ok(axum::http::StatusCode::ACCEPTED)
}
