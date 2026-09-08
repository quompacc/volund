use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
    RANGE,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::api::ApiState;
use crate::api_models::{ErrorBody, ErrorEnvelope};
use crate::catalog_queries;

#[derive(Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: u64,
}

pub async fn serve(
    State(state): State<ApiState>,
    Path((preview_id, kind)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !matches!(
        kind.as_str(),
        "preview-glb" | "thumbnail-raster" | "assembly-manifest" | "diagnostics" | "result"
    ) {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "preview artifact not found",
        );
    }
    let record = match catalog_queries::get_artifact(&state.pool, &preview_id, &kind).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "preview artifact not found",
            );
        }
        Err(diagnostic) => {
            eprintln!("API artifact database error: {diagnostic}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "preview artifact unavailable",
            );
        }
    };
    serve_record(&state, record, &headers).await
}

pub async fn serve_by_id(
    State(state): State<ApiState>,
    Path(artifact_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let record = match catalog_queries::get_artifact_by_id(&state.pool, &artifact_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "artifact not found");
        }
        Err(diagnostic) => {
            eprintln!("API artifact database error: {diagnostic}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "artifact unavailable",
            );
        }
    };
    serve_record(&state, record, &headers).await
}

async fn serve_record(
    state: &ApiState,
    record: catalog_queries::ArtifactRecord,
    headers: &HeaderMap,
) -> Response {
    let root = match tokio::fs::canonicalize(&state.derived_root).await {
        Ok(path) => path,
        Err(error) => return artifact_failure(&format!("cannot resolve derived root: {error}")),
    };
    let path = root.join(PathBuf::from(&record.relative_path));
    let canonical = match tokio::fs::canonicalize(&path).await {
        Ok(path) if path.starts_with(&root) => path,
        Ok(_) => return artifact_failure("artifact escaped derived root"),
        Err(error) => return artifact_failure(&format!("cannot resolve artifact: {error}")),
    };
    stream_verified_file(
        &canonical,
        record.byte_size,
        &record.sha256,
        &record.media_type,
        "public, max-age=31536000, immutable",
        headers,
    )
    .await
    .unwrap_or_else(|message| artifact_failure(&message))
}

pub(crate) async fn stream_verified_file(
    path: &std::path::Path,
    expected_size: i64,
    sha256: &str,
    media_type: &str,
    cache_control: &str,
    headers: &HeaderMap,
) -> Result<Response, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("cannot open file: {error}"))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|error| format!("cannot inspect file: {error}"))?;
    let actual_size = metadata.len();
    if !metadata.is_file() {
        return Err("stream source is not a regular file".to_owned());
    }
    if i64::try_from(actual_size).ok() != Some(expected_size) {
        return Err("file size differs from catalog metadata".to_owned());
    }
    // Verify the same open file that will be streamed, before conditional/range
    // responses. A catalog hash is not evidence that on-disk bytes are unchanged.
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut read_bytes = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("cannot verify file: {error}"))?;
        if count == 0 {
            break;
        }
        read_bytes += count as u64;
        if read_bytes > actual_size {
            return Err("file grew during verification".to_owned());
        }
        digest.update(&buffer[..count]);
    }
    if read_bytes != actual_size || format!("{:x}", digest.finalize()) != sha256 {
        return Err("file hash differs from catalog metadata".to_owned());
    }
    let etag = format!("\"{sha256}\"");
    if headers
        .get(IF_NONE_MATCH)
        .is_some_and(|value| value.as_bytes() == etag.as_bytes())
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(ETAG, etag)
            .header(CACHE_CONTROL, cache_control)
            .body(Body::empty())
            .map_err(|error| format!("cannot build response: {error}"));
    }
    let requested_range = headers.get(RANGE).map(|value| value.to_str());
    let range = match requested_range {
        None => None,
        Some(Ok(value)) => match parse_range(value, actual_size) {
            Ok(range) => Some(range),
            Err(()) => return Ok(range_error(actual_size)),
        },
        Some(Err(_)) => return Ok(range_error(actual_size)),
    };
    let (status, start, end) = range.map_or(
        (StatusCode::OK, 0, actual_size.saturating_sub(1)),
        |range| (StatusCode::PARTIAL_CONTENT, range.start, range.end),
    );
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return Err("cannot seek file".to_owned());
    }
    let length = if actual_size == 0 { 0 } else { end - start + 1 };
    let stream = ReaderStream::new(file.take(length));
    let mut builder = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, media_type)
        .header(CONTENT_LENGTH, length)
        .header(ACCEPT_RANGES, "bytes")
        .header(CACHE_CONTROL, cache_control)
        .header(ETAG, etag);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(CONTENT_RANGE, format!("bytes {start}-{end}/{actual_size}"));
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|error| format!("cannot build response: {error}"))
}

fn parse_range(value: &str, size: u64) -> Result<ByteRange, ()> {
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if size == 0 || value.contains(',') {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(ByteRange {
            start: size.saturating_sub(suffix),
            end: size - 1,
        });
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    if start >= size {
        return Err(());
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(size - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(ByteRange { start, end })
}

fn range_error(size: u64) -> Response {
    let mut response = error_response(
        StatusCode::RANGE_NOT_SATISFIABLE,
        "range_not_satisfiable",
        "requested byte range is not satisfiable",
    );
    response.headers_mut().insert(
        CONTENT_RANGE,
        format!("bytes */{size}")
            .parse()
            .expect("valid content range"),
    );
    response
}

fn artifact_failure(diagnostic: &str) -> Response {
    eprintln!("API artifact error: {diagnostic}");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "preview artifact unavailable",
    )
}

fn error_response(status: StatusCode, code: &'static str, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code,
                message: message.to_owned(),
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_http_ranges_are_normalized() {
        assert_eq!(
            parse_range("bytes=2-5", 10),
            Ok(ByteRange { start: 2, end: 5 })
        );
        assert_eq!(
            parse_range("bytes=7-", 10),
            Ok(ByteRange { start: 7, end: 9 })
        );
        assert_eq!(
            parse_range("bytes=-3", 10),
            Ok(ByteRange { start: 7, end: 9 })
        );
        assert_eq!(
            parse_range("bytes=8-99", 10),
            Ok(ByteRange { start: 8, end: 9 })
        );
    }

    #[test]
    fn malformed_multiple_and_unsatisfiable_ranges_are_rejected() {
        assert!(parse_range("items=0-1", 10).is_err());
        assert!(parse_range("bytes=0-1,3-4", 10).is_err());
        assert!(parse_range("bytes=10-", 10).is_err());
        assert!(parse_range("bytes=5-2", 10).is_err());
        assert!(parse_range("bytes=-0", 10).is_err());
        assert!(parse_range("bytes=0-0", 0).is_err());
    }
}
