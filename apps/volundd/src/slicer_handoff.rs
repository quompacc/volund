use axum::Json;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use chrono::{Duration, Utc};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::fmt::Write;

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_models::{
    CreateSlicerHandoffRequest, ErrorBody, ErrorEnvelope, SlicerHandoffResponse,
    SlicerTargetSummary,
};
use crate::session::AuthenticatedSession;

const TARGETS_ENV: &str = "VOLUND_SLICER_TARGETS";
const PUBLIC_BASE_ENV: &str = "VOLUND_PUBLIC_BASE_URL";
const ALLOWED_SCHEMES: &[&str] = &["prusaslicer", "orcaslicer", "bambustudio"];

#[derive(Clone, Debug)]
struct Target {
    id: String,
    name: String,
    scheme: String,
}

/// List operator-configured slicers without exposing a command or host path.
///
/// # Errors
/// Returns a configuration error when a configured target violates the allowlist.
pub(crate) fn targets() -> Result<Vec<SlicerTargetSummary>, ApiError> {
    configured_targets().map(|targets| {
        targets
            .into_iter()
            .map(|target| SlicerTargetSummary {
                id: target.id,
                name: target.name,
                scheme: target.scheme,
            })
            .collect()
    })
}

/// Create a five-minute bearer URL for one explicitly printable STL or 3MF.
///
/// # Errors
/// Returns bounded configuration, ownership, file-kind, or database errors.
pub(crate) async fn create(
    state: &ApiState,
    actor: &AuthenticatedSession,
    model_id: &str,
    file_id: &str,
    request: CreateSlicerHandoffRequest,
) -> Result<SlicerHandoffResponse, ApiError> {
    let target = configured_targets()?
        .into_iter()
        .find(|target| target.id == request.target_id)
        .ok_or_else(|| ApiError::BadRequest("unknown slicer target".to_owned()))?;
    let base = public_base()?;
    let row = sqlx::query(
        "SELECT model.id,source.id,source.relative_path,link.printable,source.missing_at IS NOT NULL \
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         WHERE model.public_id::text=$1 AND source.public_id::text=$2 \
         AND model.active AND source.lifecycle_state='available'",
    )
    .bind(model_id)
    .bind(file_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot resolve slicer source: {error}")))?
    .ok_or_else(|| ApiError::NotFound("model file relationship not found".to_owned()))?;
    let path: String = row.get(2);
    if !row.get::<bool, _>(3) || row.get::<bool, _>(4) || !is_slicer_file(&path) {
        return Err(ApiError::BadRequest(
            "slicer handoff requires an available printable STL or 3MF".to_owned(),
        ));
    }
    let mut token_bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    let digest = hex::encode(Sha256::digest(token.as_bytes()));
    let expires = Utc::now() + Duration::minutes(5);
    sqlx::query("DELETE FROM volund.slicer_handoffs WHERE expires_at <= now()")
        .execute(&state.pool)
        .await
        .map_err(|error| ApiError::Database(format!("cannot expire slicer handoffs: {error}")))?;
    let mut transaction = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::Database(format!("cannot start slicer handoff: {error}")))?;
    sqlx::query(
        "INSERT INTO volund.slicer_handoffs \
         (token_digest,model_id,source_file_id,actor_user_id,target_id,expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(digest)
    .bind(row.get::<i64, _>(0))
    .bind(row.get::<i64, _>(1))
    .bind(actor.database_user_id())
    .bind(&target.id)
    .bind(expires)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ApiError::Database(format!("cannot create slicer handoff: {error}")))?;
    crate::catalog_audit::record(
        &mut transaction,
        actor,
        "model.file.slicer-handoff.create",
        "source-file",
        file_id,
        serde_json::json!({"modelId":model_id,"targetId":&target.id,"expiresAtUnixMs":expires.timestamp_millis()}),
    )
    .await
    .map_err(|error| ApiError::Database(format!("cannot audit slicer handoff: {error}")))?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::Database(format!("cannot commit slicer handoff: {error}")))?;
    let download_url = format!("{base}/api/v1/slicer-download/{token}");
    let launch_url = format!(
        "{}://open?file={}",
        target.scheme,
        percent_encode(&download_url)
    );
    Ok(SlicerHandoffResponse {
        target_id: target.id,
        launch_url,
        download_url,
        expires_at_unix_ms: expires.timestamp_millis(),
    })
}

/// Stream a valid bearer handoff as an attachment.
pub(crate) async fn download(state: ApiState, token: String, headers: HeaderMap) -> Response {
    let digest = hex::encode(Sha256::digest(token.as_bytes()));
    let file_id = sqlx::query_scalar(
        "SELECT source.public_id::text FROM volund.slicer_handoffs handoff \
         JOIN volund.source_files source ON source.id=handoff.source_file_id \
         WHERE handoff.token_digest=$1 AND handoff.expires_at > now()",
    )
    .bind(digest)
    .fetch_optional(&state.pool)
    .await;
    match file_id {
        Ok(Some(file_id)) => crate::source_stream::serve_download(state, file_id, headers).await,
        Ok(None) => handoff_error(StatusCode::NOT_FOUND, "slicer handoff is unavailable"),
        Err(error) => {
            eprintln!("slicer handoff lookup failed: {error}");
            handoff_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "slicer handoff is unavailable",
            )
        }
    }
}

fn configured_targets() -> Result<Vec<Target>, ApiError> {
    let value = std::env::var(TARGETS_ENV).unwrap_or_default();
    parse_targets(&value)
}

fn parse_targets(value: &str) -> Result<Vec<Target>, ApiError> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let targets: Vec<Target> = value
        .split(',')
        .map(|entry| {
            let parts: Vec<_> = entry.split('|').map(str::trim).collect();
            if parts.len() != 3
                || parts[0].is_empty()
                || parts[0].len() > 32
                || !valid_target_id(parts[0])
                || parts[1].is_empty()
                || parts[1].chars().count() > 80
                || parts[1].chars().any(char::is_control)
                || !ALLOWED_SCHEMES.contains(&parts[2])
            {
                return Err(ApiError::BadRequest(
                    "VOLUND_SLICER_TARGETS contains an invalid target".to_owned(),
                ));
            }
            Ok(Target {
                id: parts[0].to_owned(),
                name: parts[1].to_owned(),
                scheme: parts[2].to_owned(),
            })
        })
        .collect::<Result<_, _>>()?;
    let mut ids = std::collections::HashSet::with_capacity(targets.len());
    if targets.iter().any(|target| !ids.insert(&target.id)) {
        return Err(ApiError::BadRequest(
            "VOLUND_SLICER_TARGETS contains duplicate target IDs".to_owned(),
        ));
    }
    Ok(targets)
}

fn valid_target_id(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn public_base() -> Result<String, ApiError> {
    let value = std::env::var(PUBLIC_BASE_ENV).unwrap_or_default();
    parse_public_base(&value)
}

fn parse_public_base(value: &str) -> Result<String, ApiError> {
    let value = value.trim().trim_end_matches('/');
    let uri = value.parse::<Uri>().ok();
    let valid = uri.as_ref().is_some_and(|uri| {
        uri.scheme_str() == Some("https")
            && uri.authority().is_some_and(|authority| {
                !authority.host().is_empty() && !authority.as_str().contains('@')
            })
            && uri.path() == "/"
            && uri.query().is_none()
    });
    if !valid || value.len() > 300 || value.chars().any(char::is_whitespace) || value.contains('#')
    {
        return Err(ApiError::BadRequest(
            "VOLUND_PUBLIC_BASE_URL must be an absolute HTTPS origin".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn is_slicer_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("stl") || value.eq_ignore_ascii_case("3mf"))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

fn handoff_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code: "not_found",
                message: message.to_owned(),
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{is_slicer_file, parse_public_base, parse_targets, percent_encode};

    #[test]
    fn slicer_files_and_nested_url_encoding_are_strict() {
        assert!(is_slicer_file("parts/body.STL"));
        assert!(is_slicer_file("plate.3mf"));
        assert!(!is_slicer_file("assembly.step"));
        assert_eq!(
            percent_encode("https://vault.test/a?b=c&d=e"),
            "https%3A%2F%2Fvault.test%2Fa%3Fb%3Dc%26d%3De"
        );
    }

    #[test]
    fn slicer_targets_reject_commands_controls_and_ambiguity() {
        let Ok(targets) =
            parse_targets("prusaslicer|PrusaSlicer|prusaslicer,orca-2|Orca Slicer|orcaslicer")
        else {
            panic!("allowlisted slicer targets were rejected");
        };
        assert_eq!(targets.len(), 2);
        for invalid in [
            "evil|Shell|file",
            "evil|Shell|https",
            "../evil|Shell|prusaslicer",
            "UPPER|Shell|prusaslicer",
            "evil|Line\nBreak|prusaslicer",
            "same|First|prusaslicer,same|Second|orcaslicer",
            "evil|Shell|prusaslicer|--execute",
        ] {
            assert!(parse_targets(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn public_base_is_a_bounded_https_origin() {
        let Ok(origin) = parse_public_base(" https://models.example.test:8443/ ") else {
            panic!("valid HTTPS origin was rejected");
        };
        assert_eq!(origin, "https://models.example.test:8443");
        for invalid in [
            "http://models.example.test",
            "https://models.example.test/path",
            "https://models.example.test?token=secret",
            "https://models.example.test#fragment",
            "https://user@models.example.test",
            "https://models.example.test bad",
            "prusaslicer://open",
        ] {
            assert!(parse_public_base(invalid).is_err(), "accepted {invalid:?}");
        }
    }
}
