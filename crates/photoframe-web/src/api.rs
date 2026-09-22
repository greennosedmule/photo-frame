//! JSON endpoints: manifest, tags, status, favourites, tagging, curation and
//! the requests the indexer acts on.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use photoframe_store::{RequestKind, is_hash, normalize_ts};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::app::AppState;
use crate::problem::Problem;

/// `Json` with RFC 9457 rejections.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = Problem;

    async fn from_request(req: Request, state: &S) -> Result<Self, Problem> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Self(v)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(Problem::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Media Type",
            )),
            Err(e) => Err(Problem::bad_request(e.body_text())),
        }
    }
}

fn photo_hash(hash: &str) -> Result<&str, Problem> {
    if is_hash(hash) {
        Ok(hash)
    } else {
        Err(Problem::not_found())
    }
}

const NO_STORE: (header::HeaderName, &str) = (header::CACHE_CONTROL, "no-store");

pub async fn status(State(s): State<AppState>) -> Result<Response, Problem> {
    Ok(([NO_STORE], Json(s.store.status().await?)).into_response())
}

/// Generated per request. The ETag is derived from `generation`, so an
/// unchanged library answers 304 without building the manifest at all.
pub async fn manifest(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, Problem> {
    let etag = |g: i64| format!("W/\"g{g}\"");
    let current = etag(s.store.generation().await?);
    let matches = |tag: &str| {
        headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|t| t.trim() == tag))
    };
    if matches(&current) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, current),
                (header::CACHE_CONTROL, "no-cache".into()),
            ],
        )
            .into_response());
    }
    let m = s.store.manifest().await?;
    Ok((
        [
            (header::ETAG, etag(m.generation)),
            (header::CACHE_CONTROL, "no-cache".into()),
        ],
        Json(m),
    )
        .into_response())
}

pub async fn tags(State(s): State<AppState>) -> Result<Response, Problem> {
    Ok(([NO_STORE], Json(s.store.list_tags().await?)).into_response())
}

pub async fn toggle_favorite(
    State(s): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Response, Problem> {
    match s.store.toggle_favorite(photo_hash(&hash)?).await? {
        Some(favorite) => Ok(Json(json!({ "favorite": favorite })).into_response()),
        None => Err(Problem::not_found()),
    }
}

/// Trim, and refuse empty, overlong or control-character names.
fn tag_name(raw: &str) -> Result<String, Problem> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(Problem::bad_request(
            "tag names are 1 to 64 characters, without control characters",
        ));
    }
    Ok(name.to_string())
}

#[derive(Deserialize)]
pub struct NewTag {
    name: String,
}

pub async fn create_tag(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<NewTag>,
) -> Result<Response, Problem> {
    let name = s.store.create_tag(&tag_name(&body.name)?).await?;
    Ok((StatusCode::CREATED, Json(json!({ "name": name }))).into_response())
}

#[derive(Deserialize)]
pub struct TagChange {
    add: Option<String>,
    remove: Option<String>,
}

pub async fn change_photo_tag(
    State(s): State<AppState>,
    Path(hash): Path<String>,
    ApiJson(body): ApiJson<TagChange>,
) -> Result<Response, Problem> {
    let hash = photo_hash(&hash)?;
    let found = match (&body.add, &body.remove) {
        (Some(name), None) => s.store.add_photo_tag(hash, &tag_name(name)?).await?,
        (None, Some(name)) => s.store.remove_photo_tag(hash, &tag_name(name)?).await?,
        _ => {
            return Err(Problem::bad_request(
                "send exactly one of \"add\" or \"remove\"",
            ));
        }
    };
    if found {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(Problem::not_found())
    }
}

/// `{"date_override": "<RFC 3339>" | null}` sets or clears the date, and
/// `{"rotate": 90}` turns the photograph a quarter turn clockwise per 90 (any
/// non-zero multiple of 90 from -270 to 270). One or both may be sent. Rotation
/// is relative so batched and repeated requests compose.
pub async fn patch_photo(
    State(s): State<AppState>,
    Path(hash): Path<String>,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response, Problem> {
    let hash = photo_hash(&hash)?;
    let date =
        match body.get("date_override") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(d)) => Some(Some(normalize_ts(d).ok_or_else(|| {
                Problem::bad_request("date_override must be an RFC 3339 timestamp")
            })?)),
            Some(_) => {
                return Err(Problem::bad_request(
                    "date_override must be a timestamp or null",
                ));
            }
        };
    let rotate = match body.get("rotate") {
        None => None,
        Some(v) => match v.as_i64().and_then(|n| i32::try_from(n).ok()) {
            Some(n) if n != 0 && n % 90 == 0 && (-270..=270).contains(&n) => Some(n),
            _ => {
                return Err(Problem::bad_request(
                    "rotate must be a non-zero multiple of 90 between -270 and 270",
                ));
            }
        },
    };
    if date.is_none() && rotate.is_none() {
        return Err(Problem::bad_request(
            "send date_override (a timestamp, or null to clear) and/or rotate",
        ));
    }
    if let Some(date) = date
        && !s.store.set_date_override(hash, date.as_deref()).await?
    {
        return Err(Problem::not_found());
    }
    if let Some(delta) = rotate {
        if s.store.rotate_photo(hash, delta).await?.is_none() {
            return Err(Problem::not_found());
        }
        // The indexer regenerates derivatives; do not make the user wait a poll.
        s.store.request_scan().await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The web tier never touches `library/`: it records the request and the
/// indexer performs the deletion. The photograph disappears from the manifest
/// immediately.
pub async fn delete_photo(
    State(s): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Response, Problem> {
    let hash = photo_hash(&hash)?;
    match s
        .store
        .enqueue_request(RequestKind::Delete, Some(hash))
        .await
    {
        Ok(id) => Ok((StatusCode::ACCEPTED, Json(json!({ "request": id }))).into_response()),
        Err(photoframe_store::Error::Invalid(_)) => Err(Problem::not_found()),
        Err(e) => Err(e.into()),
    }
}

pub async fn request_export(State(s): State<AppState>) -> Result<Response, Problem> {
    let id = s.store.enqueue_request(RequestKind::Export, None).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "request": id }))).into_response())
}

pub async fn get_request(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, Problem> {
    match s.store.get_request(id).await? {
        Some(r) => Ok(([NO_STORE], Json(r)).into_response()),
        None => Err(Problem::not_found()),
    }
}

/// Download a finished export. Names are fixed-shape, so nothing user-supplied
/// reaches the filesystem.
pub async fn download_export(
    State(s): State<AppState>,
    Path(name): Path<String>,
    req: Request,
) -> Result<Response, Problem> {
    let ok = name
        .strip_prefix("curation-")
        .and_then(|n| n.strip_suffix(".json"))
        .is_some_and(|mid| {
            !mid.is_empty()
                && mid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'T' || b == b'Z')
        });
    if !ok {
        return Err(Problem::not_found());
    }
    let res = ServeFile::new(s.layout.exports().join(&name))
        .oneshot(req)
        .await
        .map_err(|_| Problem::internal())?;
    if res.status() == StatusCode::NOT_FOUND {
        return Err(Problem::not_found());
    }
    let mut res = res.map(axum::body::Body::new);
    res.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{name}\"")
            .parse()
            .map_err(|_| Problem::internal())?,
    );
    Ok(res)
}
