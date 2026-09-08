use std::net::IpAddr;

use rand_core::{OsRng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use subtle::ConstantTimeEq;

use crate::credential_guard::dummy_password_hash;
use crate::identity::verify_password;

mod session_config;
pub use session_config::SessionConfig;

const TOKEN_BYTES: usize = 32;
const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_USER_AGENT_CHARS: usize = 512;
const MAX_FAILED_LOGINS: i32 = 5;
const LOCK_MINUTES: i32 = 15;

#[derive(Clone, Copy)]
pub struct SessionPolicy {
    idle_minutes: i32,
    absolute_minutes: i32,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            idle_minutes: 8 * 60,
            absolute_minutes: 30 * 24 * 60,
        }
    }
}

impl SessionPolicy {
    #[must_use]
    pub(crate) fn from_minutes(idle_minutes: i32, absolute_minutes: i32) -> Self {
        Self {
            idle_minutes,
            absolute_minutes,
        }
    }
}

pub struct LoginInput {
    pub email: String,
    pub password: String,
    pub client_address: Option<IpAddr>,
    pub user_agent: Option<String>,
}

pub struct LoginSession {
    pub session: SessionSummary,
    pub session_token: String,
    pub csrf_token: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub idle_expires_at_unix_ms: i64,
    pub absolute_expires_at_unix_ms: i64,
    pub must_change_password: bool,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnSessionSummary {
    pub id: String,
    pub current: bool,
    pub created_at_unix_ms: i64,
    pub last_seen_at_unix_ms: i64,
    pub idle_expires_at_unix_ms: i64,
    pub absolute_expires_at_unix_ms: i64,
    pub client_address: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone)]
pub struct AuthenticatedSession {
    database_user_id: i64,
    csrf_digest: String,
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub must_change_password: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    CatalogRead,
    CatalogWrite,
    UserAdmin,
    SettingsAdmin,
    LibraryAdmin,
    MetadataAdmin,
}

impl AuthenticatedSession {
    #[must_use]
    pub fn csrf_matches(&self, candidate: &str) -> bool {
        self.csrf_digest
            .as_bytes()
            .ct_eq(token_digest(candidate).as_bytes())
            .into()
    }

    #[must_use]
    pub fn has_capability(&self, capability: Capability) -> bool {
        matches!(
            (self.role.as_str(), capability),
            (
                "owner" | "administrator" | "editor" | "viewer",
                Capability::CatalogRead
            ) | (
                "owner" | "administrator" | "editor",
                Capability::CatalogWrite
            ) | (
                "owner" | "administrator",
                Capability::UserAdmin
                    | Capability::SettingsAdmin
                    | Capability::LibraryAdmin
                    | Capability::MetadataAdmin
            )
        )
    }

    #[must_use]
    pub(crate) fn database_user_id(&self) -> i64 {
        self.database_user_id
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SessionError {
    InvalidCredentials,
    InvalidSession,
    InvalidCsrf,
    NotFound,
    Unavailable(String),
}

/// Verify credentials and create a revocable opaque browser session.
///
/// # Errors
///
/// Returns a generic credential error for all user-visible authentication
/// failures, or an unavailable error when persistence fails.
pub async fn login(
    pool: &PgPool,
    input: LoginInput,
    policy: SessionPolicy,
) -> Result<LoginSession, SessionError> {
    let normalized_email = input.email.trim().to_lowercase();
    let account = sqlx::query(
        "SELECT account.id, account.public_id::text, account.email, \
         account.display_name, account.role, account.status, \
         account.locked_until IS NOT NULL AND account.locked_until > now(), \
         credential.password_hash, credential.must_change \
         FROM volund.users account \
         LEFT JOIN volund.password_credentials credential ON credential.user_id = account.id \
         WHERE account.normalized_email = $1",
    )
    .bind(&normalized_email)
    .fetch_optional(pool)
    .await
    .map_err(database_error("load login account"))?;
    let encoded_hash = account
        .as_ref()
        .and_then(|row| row.try_get::<String, _>(7).ok());
    let password = input.password;
    let password_valid = if password.len() <= MAX_PASSWORD_BYTES {
        tokio::task::spawn_blocking(move || {
            let encoded_hash = encoded_hash.unwrap_or_else(dummy_password_hash);
            verify_password(&password, &encoded_hash)
        })
        .await
        .map_err(|error| {
            SessionError::Unavailable(format!("password verification task failed: {error}"))
        })?
    } else {
        false
    };
    let account_usable = account
        .as_ref()
        .is_some_and(|row| row.get::<String, _>(5) == "active" && !row.get::<bool, _>(6));
    if !password_valid || !account_usable {
        record_failed_login(pool, account.as_ref(), input.client_address).await?;
        return Err(SessionError::InvalidCredentials);
    }
    let account = account.ok_or(SessionError::InvalidCredentials)?;
    create_session(
        pool,
        &account,
        input.client_address,
        input.user_agent,
        policy,
    )
    .await
}

/// Resolve and refresh one active opaque session token.
///
/// # Errors
///
/// Returns `InvalidSession` for malformed, expired, revoked, or disabled-user
/// sessions and `Unavailable` for database failures.
pub async fn authenticate(
    pool: &PgPool,
    session_token: &str,
    policy: SessionPolicy,
) -> Result<AuthenticatedSession, SessionError> {
    if session_token.len() != TOKEN_BYTES * 2
        || !session_token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SessionError::InvalidSession);
    }
    let row = sqlx::query(
        "UPDATE volund.sessions session SET \
         last_seen_at = now(), \
         idle_expires_at = LEAST(session.absolute_expires_at, \
             now() + make_interval(mins => $2)) \
         FROM volund.users account, volund.password_credentials credential \
         WHERE session.user_id = account.id AND session.token_digest = $1 \
         AND credential.user_id = account.id \
         AND session.revoked_at IS NULL AND session.idle_expires_at > now() \
         AND session.absolute_expires_at > now() AND account.status = 'active' \
         RETURNING session.public_id::text, session.csrf_digest, account.id, \
         account.public_id::text, account.email, account.display_name, account.role, \
         credential.must_change",
    )
    .bind(token_digest(session_token))
    .bind(policy.idle_minutes)
    .fetch_optional(pool)
    .await
    .map_err(database_error("authenticate session"))?
    .ok_or(SessionError::InvalidSession)?;
    Ok(AuthenticatedSession {
        session_id: row.get(0),
        csrf_digest: row.get(1),
        database_user_id: row.get(2),
        user_id: row.get(3),
        email: row.get(4),
        display_name: row.get(5),
        role: row.get(6),
        must_change_password: row.get(7),
    })
}

/// Revoke the authenticated session immediately and audit the action.
///
/// # Errors
///
/// Returns an error when the revocation transaction cannot be completed.
pub async fn revoke_current(
    pool: &PgPool,
    actor: &AuthenticatedSession,
) -> Result<(), SessionError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin session revocation"))?;
    sqlx::query(
        "UPDATE volund.sessions SET revoked_at = now(), revocation_reason = 'logout' \
         WHERE public_id::text = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(&actor.session_id)
    .bind(actor.database_user_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("revoke session"))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id) \
         VALUES ($1, $2::uuid, $3, 'session.logout', 'success', 'session', $4::uuid)",
    )
    .bind(actor.database_user_id)
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(&actor.session_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit session revocation"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit session revocation"))
}

/// List the authenticated user's currently usable browser sessions.
///
/// # Errors
///
/// Returns an unavailable error when the session inventory cannot be loaded.
pub async fn list_own(
    pool: &PgPool,
    actor: &AuthenticatedSession,
) -> Result<Vec<OwnSessionSummary>, SessionError> {
    let rows = sqlx::query(
        "SELECT public_id::text, public_id::text = $2, \
         (extract(epoch FROM created_at) * 1000)::bigint, \
         (extract(epoch FROM last_seen_at) * 1000)::bigint, \
         (extract(epoch FROM idle_expires_at) * 1000)::bigint, \
         (extract(epoch FROM absolute_expires_at) * 1000)::bigint, \
         host(client_address), user_agent \
         FROM volund.sessions WHERE user_id = $1 AND revoked_at IS NULL \
         AND idle_expires_at > now() AND absolute_expires_at > now() \
         ORDER BY created_at DESC",
    )
    .bind(actor.database_user_id)
    .bind(&actor.session_id)
    .fetch_all(pool)
    .await
    .map_err(database_error("list own sessions"))?;
    Ok(rows
        .into_iter()
        .map(|row| OwnSessionSummary {
            id: row.get(0),
            current: row.get(1),
            created_at_unix_ms: row.get(2),
            last_seen_at_unix_ms: row.get(3),
            idle_expires_at_unix_ms: row.get(4),
            absolute_expires_at_unix_ms: row.get(5),
            client_address: row.get(6),
            user_agent: row.get(7),
        })
        .collect())
}

/// Revoke one other session owned by the authenticated user.
///
/// # Errors
///
/// Returns `NotFound` for a missing, foreign, current, or already revoked
/// session, and unavailable when the transaction cannot be completed.
pub async fn revoke_own(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    session_id: &str,
) -> Result<(), SessionError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin own-session revocation"))?;
    let result = sqlx::query(
        "UPDATE volund.sessions SET revoked_at = now(), revocation_reason = 'user-revoked' \
         WHERE public_id::text = $1 AND user_id = $2 AND public_id::text <> $3 \
         AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(actor.database_user_id)
    .bind(&actor.session_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("revoke own session"))?;
    if result.rows_affected() != 1 {
        return Err(SessionError::NotFound);
    }
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id) \
         VALUES ($1, $2::uuid, $3, 'session.revoke', 'success', 'session', $4::uuid)",
    )
    .bind(actor.database_user_id)
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(session_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit own-session revocation"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit own-session revocation"))
}

async fn create_session(
    pool: &PgPool,
    account: &sqlx::postgres::PgRow,
    client_address: Option<IpAddr>,
    user_agent: Option<String>,
    policy: SessionPolicy,
) -> Result<LoginSession, SessionError> {
    let session_token = random_token();
    let csrf_token = random_token();
    let user_id: i64 = account.get(0);
    let public_user_id: String = account.get(1);
    let email: String = account.get(2);
    let display_name: String = account.get(3);
    let role: String = account.get(4);
    let must_change_password: bool = account.get(8);
    let user_agent: Option<String> =
        user_agent.map(|value| value.chars().take(MAX_USER_AGENT_CHARS).collect());
    let client_address = client_address.map(|address| address.to_string());
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin login session"))?;
    if !crate::credential_guard::lock_current(
        &mut transaction,
        user_id,
        &account.get::<String, _>(7),
    )
    .await
    .map_err(database_error("recheck login credential"))?
    {
        return Err(SessionError::InvalidCredentials);
    }
    let row = sqlx::query(
        "INSERT INTO volund.sessions \
         (user_id, token_digest, csrf_digest, idle_expires_at, absolute_expires_at, \
          client_address, user_agent) \
         VALUES ($1, $2, $3, now() + make_interval(mins => $4), \
          now() + make_interval(mins => $5), $6::inet, $7) \
         RETURNING public_id::text, \
         (extract(epoch FROM idle_expires_at) * 1000)::bigint, \
         (extract(epoch FROM absolute_expires_at) * 1000)::bigint",
    )
    .bind(user_id)
    .bind(token_digest(&session_token))
    .bind(token_digest(&csrf_token))
    .bind(policy.idle_minutes)
    .bind(policy.absolute_minutes)
    .bind(client_address.as_deref())
    .bind(user_agent)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("create login session"))?;
    sqlx::query(
        "UPDATE volund.users SET failed_login_count = 0, locked_until = NULL, \
         last_login_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("record successful login"))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id, client_address) \
         VALUES ($1, $2::uuid, $3, 'session.login', 'success', 'session', $4::uuid, $5::inet)",
    )
    .bind(user_id)
    .bind(&public_user_id)
    .bind(&display_name)
    .bind(row.get::<String, _>(0))
    .bind(client_address.as_deref())
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit successful login"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit login session"))?;
    Ok(LoginSession {
        session: SessionSummary {
            id: row.get(0),
            user_id: public_user_id,
            email,
            display_name,
            role,
            idle_expires_at_unix_ms: row.get(1),
            absolute_expires_at_unix_ms: row.get(2),
            must_change_password,
        },
        session_token,
        csrf_token,
    })
}

async fn record_failed_login(
    pool: &PgPool,
    account: Option<&sqlx::postgres::PgRow>,
    client_address: Option<IpAddr>,
) -> Result<(), SessionError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin failed-login audit"))?;
    let (database_user_id, public_id, display_name) = account.map_or((None, None, None), |row| {
        (
            Some(row.get::<i64, _>(0)),
            Some(row.get::<String, _>(1)),
            Some(row.get::<String, _>(3)),
        )
    });
    if let Some(user_id) = database_user_id {
        sqlx::query(
            "UPDATE volund.users SET failed_login_count = failed_login_count + 1, \
             locked_until = CASE WHEN failed_login_count + 1 >= $2 \
             THEN now() + make_interval(mins => $3) ELSE locked_until END, \
             updated_at = now() WHERE id = $1",
        )
        .bind(user_id)
        .bind(MAX_FAILED_LOGINS)
        .bind(LOCK_MINUTES)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("record failed login"))?;
    }
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id, client_address) \
         VALUES ($1, $2::uuid, $3, 'session.login', 'denied', 'user', $2::uuid, $4::inet)",
    )
    .bind(database_user_id)
    .bind(public_id)
    .bind(display_name)
    .bind(client_address.map(|address| address.to_string()))
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit failed login"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit failed-login audit"))
}

fn random_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> SessionError {
    move |error| SessionError::Unavailable(format!("{context}: {error}"))
}
