//! Personal API tokens for automation (ADR 0037).
//!
//! A token is `vlnd_` followed by 64 hex digits of `OsRng` randomness. Only its
//! SHA-256 digest is stored. It is shown once at creation, always expires,
//! carries a scope (`read` or `catalog-write`) that never exceeds the owner's
//! role, and cannot reach account, session, or token management.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::session::{AuthenticatedSession, SessionError, token_digest};

pub const TOKEN_PREFIX: &str = "vlnd_";
pub const MAX_ACTIVE_TOKENS: i64 = 20;
pub const MAX_LIFETIME_DAYS: i32 = 365;
const DEFAULT_LIFETIME_DAYS: i32 = 90;

/// What an API token may do, always within the owning account's role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenScope {
    Read,
    CatalogWrite,
}

impl TokenScope {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "catalog-write" => Some(Self::CatalogWrite),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::CatalogWrite => "catalog-write",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scope: String,
    pub expires_in_days: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenSummary {
    pub id: String,
    pub name: String,
    /// The first eight hex digits, to recognize a token without revealing it.
    pub prefix: String,
    pub scope: String,
    pub created_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
    pub last_used_at_unix_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedToken {
    #[serde(flatten)]
    pub summary: ApiTokenSummary,
    /// The secret; it is never returned again.
    pub token: String,
}

pub(crate) fn routes() -> axum::Router<ApiState> {
    axum::Router::new()
        .route("/account/tokens", get(list).post(create))
        .route("/account/tokens/{token_id}", delete(revoke))
}

/// Read the bearer token of `Authorization`, if the header carries one.
#[must_use]
pub fn bearer(value: &str) -> Option<&str> {
    let token = value.strip_prefix("Bearer ")?.trim();
    let secret = token.strip_prefix(TOKEN_PREFIX)?;
    (secret.len() == 64
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(token)
}

/// Resolve a bearer token to its active, unexpired owner.
///
/// # Errors
/// Returns `InvalidSession` for unknown, revoked, expired, or disabled tokens.
pub async fn authenticate(
    pool: &PgPool,
    token: &str,
) -> Result<AuthenticatedSession, SessionError> {
    let row = sqlx::query(
        "UPDATE volund.api_tokens token SET last_used_at = now() FROM volund.users account \
         WHERE token.token_digest = $1 AND token.user_id = account.id AND token.revoked_at IS NULL \
         AND token.expires_at > now() AND account.status = 'active' \
         RETURNING token.public_id::text, token.scope, account.id, account.public_id::text, \
         account.email, account.display_name, account.role",
    )
    .bind(token_digest(token))
    .fetch_optional(pool)
    .await
    .map_err(|error| SessionError::Unavailable(format!("authenticate token: {error}")))?
    .ok_or(SessionError::InvalidSession)?;
    let scope = TokenScope::parse(&row.get::<String, _>(1)).ok_or(SessionError::InvalidSession)?;
    Ok(AuthenticatedSession::from_token(
        format!("token:{}", row.get::<String, _>(0)),
        row.get(2),
        (row.get(3), row.get(4), row.get(5), row.get(6)),
        scope,
    ))
}

fn summary(row: &sqlx::postgres::PgRow) -> ApiTokenSummary {
    ApiTokenSummary {
        id: row.get(0),
        name: row.get(1),
        prefix: row.get(2),
        scope: row.get(3),
        created_at_unix_ms: row.get(4),
        expires_at_unix_ms: row.get(5),
        last_used_at_unix_ms: row.get(6),
    }
}

const SUMMARY: &str = "public_id::text, name, token_prefix, scope, \
     round(extract(epoch FROM created_at)*1000)::bigint, round(extract(epoch FROM expires_at)*1000)::bigint, \
     round(extract(epoch FROM last_used_at)*1000)::bigint";

async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Json<Vec<ApiTokenSummary>>, ApiError> {
    let rows = sqlx::query(&format!(
        "SELECT {SUMMARY} FROM volund.api_tokens WHERE user_id=$1 AND revoked_at IS NULL \
         AND expires_at > now() ORDER BY created_at DESC, id DESC"
    ))
    .bind(actor.database_user_id())
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot list API tokens: {error}")))?;
    Ok(Json(rows.iter().map(summary).collect()))
}

async fn create(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<CreateTokenRequest>,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    let name = request.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "token name must be 1 to 80 printable characters".to_owned(),
        ));
    }
    let scope = TokenScope::parse(&request.scope)
        .ok_or_else(|| ApiError::BadRequest("scope must be read or catalog-write".to_owned()))?;
    if scope == TokenScope::CatalogWrite
        && !actor.has_capability(crate::session::Capability::CatalogWrite)
    {
        return Err(ApiError::Forbidden);
    }
    let days = request.expires_in_days.unwrap_or(DEFAULT_LIFETIME_DAYS);
    if !(1..=MAX_LIFETIME_DAYS).contains(&days) {
        return Err(ApiError::BadRequest(format!(
            "expiresInDays must be 1 to {MAX_LIFETIME_DAYS}"
        )));
    }
    let mut secret = [0_u8; 32];
    OsRng.fill_bytes(&mut secret);
    let secret = hex::encode(secret);
    let token = format!("{TOKEN_PREFIX}{secret}");
    let mut transaction = state
        .pool
        .begin()
        .await
        .map_err(|error| ApiError::Database(format!("cannot begin token creation: {error}")))?;
    // Serialize creations per account so the active-token bound holds.
    sqlx::query("SELECT id FROM volund.users WHERE id=$1 FOR UPDATE")
        .bind(actor.database_user_id())
        .execute(&mut *transaction)
        .await
        .map_err(|error| ApiError::Database(format!("cannot lock account: {error}")))?;
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.api_tokens WHERE user_id=$1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(actor.database_user_id())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ApiError::Database(format!("cannot count API tokens: {error}")))?;
    if active >= MAX_ACTIVE_TOKENS {
        return Err(ApiError::TooMany(
            "api_token_limit",
            format!("at most {MAX_ACTIVE_TOKENS} active API tokens per account"),
        ));
    }
    let row = sqlx::query(&format!(
        "INSERT INTO volund.api_tokens (user_id,name,token_prefix,token_digest,scope,expires_at) \
         VALUES ($1,$2,$3,$4,$5,now() + make_interval(days => $6)) RETURNING {SUMMARY}"
    ))
    .bind(actor.database_user_id())
    .bind(&name)
    .bind(&secret[..8])
    .bind(token_digest(&token))
    .bind(scope.as_str())
    .bind(days)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ApiError::Database(format!("cannot create API token: {error}")))?;
    let created = summary(&row);
    audit(
        &mut transaction,
        &actor,
        "api-token.create",
        &created.id,
        json!({"scope": scope.as_str(), "expiresInDays": days}),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::Database(format!("cannot commit API token: {error}")))?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedToken {
            summary: created,
            token,
        }),
    ))
}

async fn revoke(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(token_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut transaction =
        state.pool.begin().await.map_err(|error| {
            ApiError::Database(format!("cannot begin token revocation: {error}"))
        })?;
    let revoked = sqlx::query(
        "UPDATE volund.api_tokens SET revoked_at = now() WHERE public_id::text=$1 AND user_id=$2 \
         AND revoked_at IS NULL",
    )
    .bind(&token_id)
    .bind(actor.database_user_id())
    .execute(&mut *transaction)
    .await
    .map_err(|error| ApiError::Database(format!("cannot revoke API token: {error}")))?
    .rows_affected();
    if revoked == 0 {
        return Err(ApiError::NotFound("API token not found".to_owned()));
    }
    audit(
        &mut transaction,
        &actor,
        "api-token.revoke",
        &token_id,
        json!({}),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::Database(format!("cannot commit token revocation: {error}")))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    token_id: &str,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) \
         VALUES ($1,$2::uuid,$3,$4,'success','api-token',$5::uuid,$6)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(token_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await
    .map_err(|error| ApiError::Database(format!("cannot audit API token: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TokenScope, bearer};

    #[test]
    fn bearer_tokens_are_strictly_shaped() {
        let token = format!("vlnd_{}", "a1".repeat(32));
        assert_eq!(bearer(&format!("Bearer {token}")), Some(token.as_str()));
        for invalid in [
            format!("Basic {token}"),
            format!("Bearer {}", "a1".repeat(32)),
            format!("Bearer vlnd_{}", "A1".repeat(32)),
            format!("Bearer vlnd_{}", "a1".repeat(31)),
            format!("Bearer vlnd_{}zz", "a1".repeat(31)),
        ] {
            assert_eq!(bearer(&invalid), None, "{invalid}");
        }
        assert_eq!(TokenScope::parse("read"), Some(TokenScope::Read));
        assert_eq!(TokenScope::parse("admin"), None);
        assert_eq!(TokenScope::CatalogWrite.as_str(), "catalog-write");
    }
}
