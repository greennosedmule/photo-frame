//! `POST /api/upload`: multipart streaming into `incoming/`.
//!
//! Each file streams to `<name>.part` and is renamed on completion, so the
//! indexer (which ignores `.part` files and applies a quiet period) never sees
//! a partial file. Size is enforced while streaming, never by buffering.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::app::AppState;
use crate::problem::Problem;

/// Enough bytes to identify every supported format by magic number.
const SNIFF_LEN: usize = 16;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_prefix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Keep the client's name for humans, but nothing that could be a path.
fn safe_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        "upload".into()
    } else {
        cleaned
    }
}

pub async fn upload(State(s): State<AppState>, mut mp: Multipart) -> Result<Response, Problem> {
    let dir = s.layout.incoming();
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "cannot create incoming/");
        Problem::internal()
    })?;
    let max = s.config.max_upload_bytes;
    let mut received = 0usize;

    while let Some(mut field) = mp
        .next_field()
        .await
        .map_err(|e| Problem::bad_request(e.body_text()))?
    {
        let Some(file_name) = field.file_name().map(safe_name) else {
            continue; // not a file field
        };

        // Buffer just enough to validate the magic bytes before touching disk.
        let mut head: Vec<u8> = Vec::with_capacity(SNIFF_LEN);
        let mut finished = false;
        while head.len() < SNIFF_LEN {
            match field
                .chunk()
                .await
                .map_err(|e| Problem::bad_request(e.body_text()))?
            {
                Some(c) => head.extend_from_slice(&c),
                None => {
                    finished = true;
                    break;
                }
            }
        }
        if head.is_empty() {
            continue;
        }
        if imagepipe::sniff(&head).is_none() {
            return Err(
                Problem::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported Media Type")
                    .detail(format!("{file_name} is not a supported image format")),
            );
        }

        // Each file gets its own directory, so the original name survives into
        // the library and two uploads can never collide.
        let file_dir = dir.join(unique_prefix());
        tokio::fs::create_dir_all(&file_dir).await.map_err(|e| {
            tracing::error!(error = %e, "cannot create upload directory");
            Problem::internal()
        })?;
        let final_path = file_dir.join(&file_name);
        let mut part = final_path.clone().into_os_string();
        part.push(".part");
        let part = std::path::PathBuf::from(part);
        let result = stream_to(&part, head, &mut field, finished, max).await;
        match result {
            Ok(()) => {
                tokio::fs::rename(&part, &final_path).await.map_err(|e| {
                    tracing::error!(error = %e, "cannot finalise upload");
                    Problem::internal()
                })?;
                received += 1;
            }
            Err(p) => {
                let _ = tokio::fs::remove_file(&part).await;
                let _ = tokio::fs::remove_dir(&file_dir).await;
                return Err(p);
            }
        }
    }

    if received == 0 {
        return Err(Problem::bad_request("no file in the request"));
    }
    // Do not make the user wait for the next poll to see their photographs.
    s.store.request_scan().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "received": received }))).into_response())
}

async fn stream_to(
    part: &std::path::Path,
    head: Vec<u8>,
    field: &mut axum::extract::multipart::Field<'_>,
    mut finished: bool,
    max: u64,
) -> Result<(), Problem> {
    let io = |e: std::io::Error| {
        tracing::error!(error = %e, "upload write failed");
        Problem::internal()
    };
    let mut file = tokio::fs::File::create(part).await.map_err(io)?;
    let mut total = head.len() as u64;
    if total > max {
        return Err(too_large(max));
    }
    file.write_all(&head).await.map_err(io)?;
    while !finished {
        match field
            .chunk()
            .await
            .map_err(|e| Problem::bad_request(e.body_text()))?
        {
            Some(c) => {
                total += c.len() as u64;
                if total > max {
                    return Err(too_large(max));
                }
                file.write_all(&c).await.map_err(io)?;
            }
            None => finished = true,
        }
    }
    file.flush().await.map_err(io)?;
    Ok(())
}

fn too_large(max: u64) -> Problem {
    Problem::new(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large")
        .detail(format!("files may be at most {max} bytes"))
}
