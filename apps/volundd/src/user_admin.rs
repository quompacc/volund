use serde::Serialize;
use sqlx::{PgPool, Row};

use crate::identity::{IdentityError, hash_password, verify_password};
use crate::session::AuthenticatedSession;

const MIN_PASSWORD_BYTES: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub status: String,
    pub created_at_unix_ms: i64,
    pub last_login_at_unix_ms: Option<i64>,
    pub locked_until_unix_ms: Option<i64>,
    pub must_change_password: bool,
}

pub struct CreateUserInput {
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub password: String,
}

pub struct UpdateUserInput {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum UserAdminError {
    BadRequest(String),
    Forbidden,
    NotFound,
    Conflict(String),
    Database(String),
}

/// List every local account for authorized administration.
///
/// # Errors
///
/// Returns a database error when the account inventory cannot be loaded.
pub async fn list_users(pool: &PgPool) -> Result<Vec<UserSummary>, UserAdminError> {
    let rows = sqlx::query(
        "SELECT account.public_id::text, account.email, account.display_name, account.role, \
         CASE WHEN account.status = 'active' AND account.locked_until > now() \
              THEN 'locked' ELSE account.status END, \
         (extract(epoch FROM created_at) * 1000)::bigint, \
         (extract(epoch FROM last_login_at) * 1000)::bigint, \
         CASE WHEN isfinite(account.locked_until) \
              THEN (extract(epoch FROM account.locked_until) * 1000)::bigint END, \
         coalesce(credential.must_change, false) \
         FROM volund.users account LEFT JOIN volund.password_credentials credential \
         ON credential.user_id = account.id ORDER BY account.display_name, account.email",
    )
    .fetch_all(pool)
    .await
    .map_err(database_error("list users"))?;
    Ok(rows.iter().map(user_summary).collect())
}

/// Create an active local account with an Argon2id credential.
///
/// # Errors
///
/// Returns validation, permission, conflict, or persistence errors.
pub async fn create_user(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: CreateUserInput,
) -> Result<UserSummary, UserAdminError> {
    let (email, normalized_email) = validate_email(&input.email)?;
    let display_name = validate_display_name(&input.display_name)?;
    validate_role(&input.role)?;
    require_owner_for_owner_role(actor, &input.role)?;
    validate_password(&input.password)?;
    let password_hash = hash_in_blocking_pool(input.password).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin user creation"))?;
    let row = sqlx::query(
        "INSERT INTO volund.users \
         (email, normalized_email, display_name, role, status, created_by_user_id) \
         VALUES ($1, $2, $3, $4, 'active', $5) ON CONFLICT (normalized_email) DO NOTHING \
         RETURNING id, public_id::text, email, display_name, role, status, \
         (extract(epoch FROM created_at) * 1000)::bigint, NULL::bigint, NULL::bigint, true",
    )
    .bind(email)
    .bind(normalized_email)
    .bind(display_name)
    .bind(&input.role)
    .bind(actor.database_user_id())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("create user"))?
    .ok_or_else(|| {
        UserAdminError::Conflict("an account with this email already exists".to_owned())
    })?;
    let database_user_id: i64 = row.get(0);
    let public_user_id: String = row.get(1);
    sqlx::query(
        "INSERT INTO volund.password_credentials (user_id, password_hash, must_change) \
         VALUES ($1, $2, true)",
    )
    .bind(database_user_id)
    .bind(password_hash)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("store user credential"))?;
    audit_user_change(&mut transaction, actor, "user.create", &public_user_id).await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit user creation"))?;
    Ok(UserSummary {
        id: public_user_id,
        email: row.get(2),
        display_name: row.get(3),
        role: row.get(4),
        status: row.get(5),
        created_at_unix_ms: row.get(6),
        last_login_at_unix_ms: row.get(7),
        locked_until_unix_ms: row.get(8),
        must_change_password: row.get(9),
    })
}

/// Update mutable account metadata while preserving owner invariants.
///
/// # Errors
///
/// Returns validation, permission, conflict, not-found, or persistence errors.
pub async fn update_user(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    user_id: &str,
    input: UpdateUserInput,
) -> Result<UserSummary, UserAdminError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin user update"))?;
    // Serialize account changes before taking any target-user row lock. Taking
    // distinct target locks first can deadlock when both updates then lock owners.
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,4)")
        .execute(&mut *transaction)
        .await
        .map_err(database_error("serialize user updates"))?;
    let target = sqlx::query(
        "SELECT id, public_id::text, display_name, role, status FROM volund.users \
         WHERE public_id::text = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("load user for update"))?
    .ok_or(UserAdminError::NotFound)?;
    let target_database_id: i64 = target.get(0);
    let target_role: String = target.get(3);
    let target_status: String = target.get(4);
    let display_name = input
        .display_name
        .as_deref()
        .map(validate_display_name)
        .transpose()?
        .unwrap_or_else(|| target.get(2));
    let role = input.role.unwrap_or_else(|| target_role.clone());
    if let Some(status) = input.status.as_deref() {
        validate_status(status)?;
    }
    let status_requested = input.status.is_some();
    let status = input.status.unwrap_or_else(|| target_status.clone());
    validate_role(&role)?;
    authorize_account_change(actor, user_id, &target_role, &role, &target_status, &status)?;
    protect_last_owner(
        &mut transaction,
        &target_role,
        &target_status,
        &role,
        &status,
    )
    .await?;
    let row = sqlx::query(
        "WITH changed AS (UPDATE volund.users SET display_name = $2, role = $3, status = $4, \
         updated_at = now(), locked_until = CASE WHEN NOT $5 THEN locked_until \
         WHEN $4 = 'active' THEN NULL \
         WHEN $4 = 'locked' THEN 'infinity'::timestamptz ELSE locked_until END \
         WHERE id = $1 RETURNING *) \
         SELECT changed.public_id::text, changed.email, changed.display_name, changed.role, \
         CASE WHEN changed.status = 'active' AND changed.locked_until > now() \
              THEN 'locked' ELSE changed.status END, \
         (extract(epoch FROM changed.created_at) * 1000)::bigint, \
         (extract(epoch FROM changed.last_login_at) * 1000)::bigint, \
         CASE WHEN isfinite(changed.locked_until) \
              THEN (extract(epoch FROM changed.locked_until) * 1000)::bigint END, \
         coalesce(credential.must_change, false) FROM changed \
         LEFT JOIN volund.password_credentials credential ON credential.user_id = changed.id",
    )
    .bind(target_database_id)
    .bind(display_name)
    .bind(&role)
    .bind(&status)
    .bind(status_requested)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("update user"))?;
    if role != target_role || status != target_status {
        revoke_user_sessions(&mut transaction, target_database_id, "account-changed").await?;
    }
    audit_user_change(&mut transaction, actor, "user.update", user_id).await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit user update"))?;
    Ok(user_summary(&row))
}

/// Replace one local credential and revoke stale sessions atomically.
///
/// # Errors
///
/// Returns validation, permission, not-found, or persistence errors.
pub async fn reset_password(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    user_id: &str,
    password: String,
    must_change: bool,
) -> Result<(), UserAdminError> {
    validate_password(&password)?;
    let password_hash = hash_in_blocking_pool(password).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin password reset"))?;
    let target =
        sqlx::query("SELECT id, role FROM volund.users WHERE public_id::text = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error("load password-reset user"))?
            .ok_or(UserAdminError::NotFound)?;
    let target_database_id: i64 = target.get(0);
    let target_role: String = target.get(1);
    if actor.role == "administrator" && target_role == "owner" {
        return Err(UserAdminError::Forbidden);
    }
    sqlx::query(
        "INSERT INTO volund.password_credentials (user_id, password_hash, must_change, changed_at) \
         VALUES ($1, $2, $3, now()) ON CONFLICT (user_id) DO UPDATE SET \
         password_hash = EXCLUDED.password_hash, must_change = EXCLUDED.must_change, changed_at = now()",
    )
    .bind(target_database_id)
    .bind(password_hash)
    .bind(must_change)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("replace user credential"))?;
    if actor.user_id == user_id {
        sqlx::query(
            "UPDATE volund.sessions SET revoked_at = now(), revocation_reason = 'password-reset' \
             WHERE user_id = $1 AND public_id::text <> $2 AND revoked_at IS NULL",
        )
        .bind(target_database_id)
        .bind(&actor.session_id)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("revoke stale self sessions"))?;
    } else {
        revoke_user_sessions(&mut transaction, target_database_id, "password-reset").await?;
    }
    audit_user_change(&mut transaction, actor, "user.password_reset", user_id).await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit password reset"))
}

/// Replace the authenticated user's credential after verifying the old secret.
///
/// # Errors
///
/// Returns validation, credential, or persistence errors. Other sessions are
/// revoked atomically while the current browser session remains usable.
pub async fn change_own_password(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    current_password: String,
    new_password: String,
) -> Result<(), UserAdminError> {
    validate_password(&new_password)?;
    if current_password.len() > MAX_PASSWORD_BYTES {
        return Err(UserAdminError::BadRequest(
            "current password is invalid".to_owned(),
        ));
    }
    let encoded_hash: String = sqlx::query_scalar(
        "SELECT password_hash FROM volund.password_credentials WHERE user_id = $1",
    )
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database_error("load own credential"))?
    .ok_or_else(|| UserAdminError::BadRequest("current password is invalid".to_owned()))?;
    let verified_hash = encoded_hash.clone();
    let password_valid =
        tokio::task::spawn_blocking(move || verify_password(&current_password, &encoded_hash))
            .await
            .map_err(|error| UserAdminError::Database(format!("password task failed: {error}")))?;
    if !password_valid {
        return Err(UserAdminError::BadRequest(
            "current password is invalid".to_owned(),
        ));
    }
    let password_hash = hash_in_blocking_pool(new_password).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin own password change"))?;
    if !crate::credential_guard::lock_current(
        &mut transaction,
        actor.database_user_id(),
        &verified_hash,
    )
    .await
    .map_err(database_error("recheck own credential"))?
    {
        return Err(UserAdminError::BadRequest(
            "credential or account changed; sign in again".to_owned(),
        ));
    }
    sqlx::query(
        "UPDATE volund.password_credentials SET password_hash = $2, must_change = false, \
         changed_at = now() WHERE user_id = $1",
    )
    .bind(actor.database_user_id())
    .bind(password_hash)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("change own credential"))?;
    sqlx::query(
        "UPDATE volund.sessions SET revoked_at = now(), revocation_reason = 'password-change' \
         WHERE user_id = $1 AND public_id::text <> $2 AND revoked_at IS NULL",
    )
    .bind(actor.database_user_id())
    .bind(&actor.session_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("revoke sessions after password change"))?;
    audit_user_change(
        &mut transaction,
        actor,
        "user.password_change",
        &actor.user_id,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit own password change"))
}

pub(crate) fn user_summary(row: &sqlx::postgres::PgRow) -> UserSummary {
    UserSummary {
        id: row.get(0),
        email: row.get(1),
        display_name: row.get(2),
        role: row.get(3),
        status: row.get(4),
        created_at_unix_ms: row.get(5),
        last_login_at_unix_ms: row.get(6),
        locked_until_unix_ms: row.get(7),
        must_change_password: row.get(8),
    }
}

pub(crate) fn validate_email(raw: &str) -> Result<(String, String), UserAdminError> {
    let email = raw.trim();
    let valid = (3..=320).contains(&email.len())
        && !email.chars().any(char::is_whitespace)
        && email.split('@').count() == 2
        && !email.starts_with('@')
        && !email.ends_with('@');
    if !valid {
        return Err(UserAdminError::BadRequest("email is invalid".to_owned()));
    }
    Ok((email.to_owned(), email.to_lowercase()))
}

pub(crate) fn validate_display_name(raw: &str) -> Result<String, UserAdminError> {
    let value = raw.trim();
    if value.is_empty() || value.chars().count() > 160 {
        return Err(UserAdminError::BadRequest(
            "display name must contain 1 to 160 characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn validate_password(password: &str) -> Result<(), UserAdminError> {
    if !(MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&password.len()) {
        return Err(UserAdminError::BadRequest(format!(
            "password must contain {MIN_PASSWORD_BYTES} to {MAX_PASSWORD_BYTES} bytes"
        )));
    }
    Ok(())
}

pub(crate) fn validate_role(role: &str) -> Result<(), UserAdminError> {
    if matches!(role, "owner" | "administrator" | "editor" | "viewer") {
        Ok(())
    } else {
        Err(UserAdminError::BadRequest("role is invalid".to_owned()))
    }
}

fn validate_status(status: &str) -> Result<(), UserAdminError> {
    if matches!(status, "active" | "disabled" | "locked") {
        Ok(())
    } else {
        Err(UserAdminError::BadRequest("status is invalid".to_owned()))
    }
}

pub(crate) fn require_owner_for_owner_role(
    actor: &AuthenticatedSession,
    role: &str,
) -> Result<(), UserAdminError> {
    if role == "owner" && actor.role != "owner" {
        Err(UserAdminError::Forbidden)
    } else {
        Ok(())
    }
}

fn authorize_account_change(
    actor: &AuthenticatedSession,
    user_id: &str,
    old_role: &str,
    new_role: &str,
    old_status: &str,
    new_status: &str,
) -> Result<(), UserAdminError> {
    if actor.user_id == user_id && (old_role != new_role || old_status != new_status) {
        return Err(UserAdminError::Forbidden);
    }
    if actor.role == "administrator" && (old_role == "owner" || new_role == "owner") {
        return Err(UserAdminError::Forbidden);
    }
    require_owner_for_owner_role(actor, new_role)
}

async fn protect_last_owner(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    old_role: &str,
    old_status: &str,
    new_role: &str,
    new_status: &str,
) -> Result<(), UserAdminError> {
    if old_role != "owner"
        || old_status != "active"
        || (new_role == "owner" && new_status == "active")
    {
        return Ok(());
    }
    sqlx::query("SELECT id FROM volund.users WHERE role = 'owner' FOR UPDATE")
        .fetch_all(&mut **transaction)
        .await
        .map_err(database_error("lock owners"))?;
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.users WHERE role = 'owner' AND status = 'active'",
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error("count active owners"))?;
    if remaining <= 1 {
        Err(UserAdminError::Conflict(
            "the last active owner cannot be disabled or demoted".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) async fn hash_in_blocking_pool(password: String) -> Result<String, UserAdminError> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| UserAdminError::Database(format!("password hash task failed: {error}")))?
        .map_err(|error| match error {
            IdentityError::Unavailable(message) => UserAdminError::Database(message),
            _ => UserAdminError::Database("password hashing failed".to_owned()),
        })
}

async fn revoke_user_sessions(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i64,
    reason: &str,
) -> Result<(), UserAdminError> {
    sqlx::query(
        "UPDATE volund.sessions SET revoked_at = now(), revocation_reason = $2 \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(reason)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("revoke user sessions"))?;
    Ok(())
}

pub(crate) async fn audit_user_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    target_user_id: &str,
) -> Result<(), UserAdminError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id) \
         VALUES ($1, $2::uuid, $3, $4, 'success', 'user', $5::uuid)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(target_user_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("audit user change"))?;
    Ok(())
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> UserAdminError {
    move |error| UserAdminError::Database(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_input_validation_is_bounded() {
        assert!(validate_email(" User@Example.test ").is_ok());
        assert!(validate_email("invalid").is_err());
        assert!(validate_display_name(" User ").is_ok());
        assert!(validate_display_name("").is_err());
        assert!(validate_role("editor").is_ok());
        assert!(validate_role("superuser").is_err());
        assert!(validate_status("disabled").is_ok());
        assert!(validate_status("deleted").is_err());
        assert!(validate_password("short").is_err());
    }
}
