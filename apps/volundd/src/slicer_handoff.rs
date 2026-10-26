use axum::Json;
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
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
/// Downloads per token, including range requests and client retries (ADR 0031).
const MAX_TOKEN_USES: i32 = 8;
/// Unexpired handoffs one account may hold at a time.
const MAX_ACTIVE_PER_ACCOUNT: i64 = 10;
const MISCONFIGURED: &str = "slicer_handoff_misconfigured";

/// Operator slicer configuration: the raw `VOLUND_SLICER_TARGETS` and
/// `VOLUND_PUBLIC_BASE_URL` values, validated on every use.
#[derive(Clone, Debug, Default)]
pub struct SlicerConfig {
    targets: String,
    public_base: String,
}

impl SlicerConfig {
    /// Read the configuration from the daemon environment.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::new(
            &std::env::var(TARGETS_ENV).unwrap_or_default(),
            &std::env::var(PUBLIC_BASE_ENV).unwrap_or_default(),
        )
    }

    /// Explicit configuration, e.g. for isolated tests.
    #[must_use]
    pub fn new(targets: &str, public_base: &str) -> Self {
        Self {
            targets: targets.to_owned(),
            public_base: public_base.to_owned(),
        }
    }
}

/// Build the default API router with an explicit slicer configuration.
pub fn router_with_config(pool: sqlx::PgPool, slicer: SlicerConfig) -> axum::Router {
    let state = (
        "/srv/volund/derived".into(),
        "/srv/volund/support".into(),
        crate::identity::IdentityConfig::default(),
        crate::session::SessionConfig::default(),
        slicer,
    );
    crate::api::router_with_state(pool, "/usr/share/volund/web".into(), state)
}

/// `GET /api/v1/slicer-targets`.
pub(crate) async fn list_targets(
    State(state): State<ApiState>,
) -> Result<Json<Vec<SlicerTargetSummary>>, ApiError> {
    targets(&state.slicer).map(Json)
}

/// `POST /api/v1/models/{model}/files/{file}/slicer-handoff`.
pub(crate) async fn create_route(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    AxumPath((model_id, file_id)): AxumPath<(String, String)>,
    Json(request): Json<CreateSlicerHandoffRequest>,
) -> Result<Json<SlicerHandoffResponse>, ApiError> {
    crate::api_auth::require_catalog_write(&actor)?;
    create(&state, &actor, &model_id, &file_id, request)
        .await
        .map(Json)
}

#[derive(Clone, Debug)]
struct Target {
    id: String,
    name: String,
    scheme: String,
}

/// List operator-configured slicers without exposing a command or host path.
///
/// # Errors
/// Returns 503 `slicer_handoff_misconfigured` when targets are configured but
/// invalid, or when no valid public HTTPS base URL exists for their downloads,
/// so clients never offer a handoff that is bound to fail.
pub(crate) fn targets(config: &SlicerConfig) -> Result<Vec<SlicerTargetSummary>, ApiError> {
    usable_targets(config).map(|targets| {
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
    let target = usable_targets(&state.slicer)?
        .into_iter()
        .find(|target| target.id == request.target_id)
        .ok_or_else(|| ApiError::BadRequest("unknown slicer target".to_owned()))?;
    let base = parse_public_base(&state.slicer.public_base).map_err(|_| misconfigured())?;
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
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.slicer_handoffs WHERE actor_user_id=$1 AND expires_at > now()",
    )
    .bind(actor.database_user_id())
    .fetch_one(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot count slicer handoffs: {error}")))?;
    if active >= MAX_ACTIVE_PER_ACCOUNT {
        return Err(ApiError::TooMany(
            "slicer_handoff_limit",
            "too many open slicer handoffs; wait a few minutes and try again".to_owned(),
        ));
    }
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
///
/// Every use re-checks what made the handoff legitimate when it was created:
/// the account is still active, the model is active, the file is still linked,
/// printable, and present, and the target is still configured. Each token
/// serves at most `MAX_TOKEN_USES` requests.
pub(crate) async fn download(state: ApiState, token: String, headers: HeaderMap) -> Response {
    let digest = hex::encode(Sha256::digest(token.as_bytes()));
    let claimed = sqlx::query_as::<_, (String, String)>(
        "UPDATE volund.slicer_handoffs handoff SET use_count=handoff.use_count+1, last_used_at=now() \
         FROM volund.source_files source, volund.models model, volund.model_source_files link, \
          volund.users account \
         WHERE handoff.token_digest=$1 AND handoff.expires_at > now() AND handoff.use_count < $2 \
         AND source.id=handoff.source_file_id AND model.id=handoff.model_id \
         AND link.model_id=model.id AND link.source_file_id=source.id \
         AND account.id=handoff.actor_user_id \
         AND model.active AND link.printable AND account.status='active' \
         AND source.lifecycle_state='available' AND source.missing_at IS NULL \
         RETURNING source.public_id::text, handoff.target_id",
    )
    .bind(digest)
    .bind(MAX_TOKEN_USES)
    .fetch_optional(&state.pool)
    .await;
    let still_configured = |target: &str| {
        usable_targets(&state.slicer)
            .is_ok_and(|targets| targets.iter().any(|item| item.id == target))
    };
    match claimed {
        Ok(Some((file_id, target))) if still_configured(&target) => {
            let mut response = crate::source_stream::serve_download(state, file_id, headers).await;
            // The URL itself is the credential: never cache it or leak it onward.
            let headers = response.headers_mut();
            headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
            response
        }
        Ok(_) => handoff_error(StatusCode::NOT_FOUND, "slicer handoff is unavailable"),
        Err(error) => {
            eprintln!("slicer handoff lookup failed: {error}");
            handoff_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "slicer handoff is unavailable",
            )
        }
    }
}

/// Targets that can actually complete a handoff: a valid allowlisted target
/// list plus a valid public HTTPS base. No targets configured is not an error.
fn usable_targets(config: &SlicerConfig) -> Result<Vec<Target>, ApiError> {
    let targets = parse_targets(&config.targets).map_err(|error| {
        eprintln!("slicer handoff disabled: {}", describe(&error));
        misconfigured()
    })?;
    if !targets.is_empty() {
        if let Err(error) = parse_public_base(&config.public_base) {
            eprintln!("slicer handoff disabled: {}", describe(&error));
            return Err(misconfigured());
        }
    }
    Ok(targets)
}

fn misconfigured() -> ApiError {
    ApiError::Unavailable(
        MISCONFIGURED,
        "slicer handoff is not fully configured on this server".to_owned(),
    )
}

fn describe(error: &ApiError) -> String {
    match error {
        ApiError::BadRequest(message) => message.clone(),
        _ => "invalid slicer configuration".to_owned(),
    }
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
