use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sqlx::Row;

use crate::api::ApiState;
use crate::api_models::{ErrorBody, ErrorEnvelope};
use crate::artifact_stream;

pub async fn serve(
    State(state): State<ApiState>,
    Path(file_id): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    stream(state, file_id, headers, query.download.unwrap_or(false)).await
}

pub(crate) async fn serve_download(
    state: ApiState,
    file_id: String,
    headers: HeaderMap,
) -> Response {
    stream(state, file_id, headers, true).await
}

async fn stream(state: ApiState, file_id: String, headers: HeaderMap, download: bool) -> Response {
    let row = match sqlx::query(
        "SELECT root.filesystem_path, source.relative_path, content.byte_size, content.sha256 \
         FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id = source.library_root_id \
         JOIN volund.content_objects content ON content.id = source.content_object_id \
         WHERE source.public_id::text = $1 AND source.lifecycle_state='available'",
    )
    .bind(&file_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "source file not found"),
        Err(error) => return stream_failure(&format!("cannot load source file: {error}")),
    };
    let root_path: String = row.get(0);
    let relative_path: String = row.get(1);
    let expected_size: i64 = row.get(2);
    let sha256: String = row.get(3);
    let root = match tokio::fs::canonicalize(&root_path).await {
        Ok(path) => path,
        Err(error) => return stream_failure(&format!("cannot resolve library root: {error}")),
    };
    let path = root.join(PathBuf::from(&relative_path));
    let canonical = match tokio::fs::canonicalize(&path).await {
        Ok(path) if path.starts_with(&root) => path,
        Ok(_) => return stream_failure("source path escaped its library root"),
        Err(_) => return error_response(StatusCode::NOT_FOUND, "source file is unavailable"),
    };
    let svg_download = relative_path.to_ascii_lowercase().ends_with(".svg");
    let mut response = artifact_stream::stream_verified_file(
        &canonical,
        expected_size,
        &sha256,
        media_type(&relative_path),
        "private, no-cache",
        &headers,
    )
    .await
    .unwrap_or_else(|message| stream_failure(&message));
    if svg_download || download {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            attachment_disposition(&relative_path),
        );
    }
    response
}

fn attachment_disposition(path: &str) -> HeaderValue {
    let filename = path.rsplit('/').next().unwrap_or("download");
    let fallback: String = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let encoded = filename
        .as_bytes()
        .iter()
        .fold(String::new(), |mut value, byte| {
            if byte.is_ascii_alphanumeric()
                || matches!(
                    *byte,
                    b'!' | b'#'
                        | b'$'
                        | b'&'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
            {
                value.push(char::from(*byte));
            } else {
                use std::fmt::Write;
                let _ = write!(value, "%{byte:02X}");
            }
            value
        });
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .expect("sanitized attachment disposition")
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamQuery {
    download: Option<bool>,
}

fn media_type(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" | "txt" | "cfg" | "ini" | "yaml" | "yml" | "gcode" => "text/plain; charset=utf-8",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "stl" => "model/stl",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn stream_failure(diagnostic: &str) -> Response {
    eprintln!("API source stream error: {diagnostic}");
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "source file unavailable")
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code: if status == StatusCode::NOT_FOUND {
                    "not_found"
                } else {
                    "internal_error"
                },
                message: message.to_owned(),
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{attachment_disposition, media_type};

    #[test]
    fn browser_media_types_cover_project_documents_and_images() {
        assert_eq!(media_type("Manual.PDF"), "application/pdf");
        assert_eq!(media_type("cover.png"), "image/png");
        assert_eq!(media_type("README.md"), "text/plain; charset=utf-8");
        assert_eq!(media_type("part.step"), "application/octet-stream");
        assert_eq!(media_type("drawing.svg"), "application/octet-stream");
    }

    #[test]
    fn downloads_preserve_safe_ascii_and_utf8_filenames() {
        assert_eq!(
            attachment_disposition("parts/fan mount.stl")
                .to_str()
                .unwrap(),
            "attachment; filename=\"fan mount.stl\"; filename*=UTF-8''fan%20mount.stl"
        );
        assert_eq!(
            attachment_disposition("Bilder/Prüfkörper.step")
                .to_str()
                .unwrap(),
            "attachment; filename=\"Pr_fk_rper.step\"; filename*=UTF-8''Pr%C3%BCfk%C3%B6rper.step"
        );
    }
}
