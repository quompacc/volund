use rand_core::{OsRng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;
use crate::user_admin::{
    UserAdminError, UserSummary, audit_user_change, hash_in_blocking_pool,
    require_owner_for_owner_role, validate_display_name, validate_email, validate_password,
    validate_role,
};

const TOKEN_BYTES: usize = 32;
const INVITATION_DAYS: i32 = 7;

pub struct InviteUserInput {
    pub email: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationCreated {
    pub user: UserSummary,
    pub activation_token: String,
    pub expires_at_unix_ms: i64,
}

/// Create an invited account and return its activation secret exactly once.
///
/// # Errors
///
/// Returns validation, permission, conflict, or persistence errors.
pub async fn invite_user(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: InviteUserInput,
) -> Result<InvitationCreated, UserAdminError> {
    let (email, normalized_email) = validate_email(&input.email)?;
    let display_name = validate_display_name(&input.display_name)?;
    validate_role(&input.role)?;
    require_owner_for_owner_role(actor, &input.role)?;
    let token = random_token();
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin user invitation"))?;
    let row = sqlx::query(
        "INSERT INTO volund.users \
         (email, normalized_email, display_name, role, status, created_by_user_id) \
         VALUES ($1, $2, $3, $4, 'invited', $5) ON CONFLICT (normalized_email) DO NOTHING \
         RETURNING id, public_id::text, email, display_name, role, status, \
         (extract(epoch FROM created_at) * 1000)::bigint, NULL::bigint, NULL::bigint, false",
    )
    .bind(email)
    .bind(normalized_email)
    .bind(display_name)
    .bind(&input.role)
    .bind(actor.database_user_id())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("create invited user"))?
    .ok_or_else(|| {
        UserAdminError::Conflict("an account with this email already exists".to_owned())
    })?;
    let database_user_id: i64 = row.get(0);
    let public_user_id: String = row.get(1);
    let expires_at_unix_ms: i64 = sqlx::query_scalar(
        "INSERT INTO volund.user_invitations \
         (user_id, token_digest, expires_at, created_by_user_id) \
         VALUES ($1, $2, now() + make_interval(days => $3), $4) \
         RETURNING (extract(epoch FROM expires_at) * 1000)::bigint",
    )
    .bind(database_user_id)
    .bind(token_digest(&token))
    .bind(INVITATION_DAYS)
    .bind(actor.database_user_id())
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("store user invitation"))?;
    audit_user_change(&mut transaction, actor, "user.invite", &public_user_id).await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit user invitation"))?;
    Ok(InvitationCreated {
        user: UserSummary {
            id: public_user_id,
            email: row.get(2),
            display_name: row.get(3),
            role: row.get(4),
            status: row.get(5),
            created_at_unix_ms: row.get(6),
            last_login_at_unix_ms: row.get(7),
            locked_until_unix_ms: row.get(8),
            must_change_password: row.get(9),
        },
        activation_token: token,
        expires_at_unix_ms,
    })
}

/// Consume a valid invitation and install the user's chosen credential.
///
/// # Errors
///
/// Returns a generic not-found response for malformed, expired, consumed, or
/// otherwise unusable invitations and validation/database errors as applicable.
pub async fn accept_invitation(
    pool: &PgPool,
    token: &str,
    password: String,
) -> Result<(), UserAdminError> {
    validate_token(token)?;
    validate_password(&password)?;
    let password_hash = hash_in_blocking_pool(password).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin invitation acceptance"))?;
    let invitation = sqlx::query(
        "SELECT invitation.id, account.id, account.public_id::text, account.display_name \
         FROM volund.user_invitations invitation \
         JOIN volund.users account ON account.id = invitation.user_id \
         WHERE invitation.token_digest = $1 AND invitation.accepted_at IS NULL \
         AND invitation.expires_at > now() AND account.status = 'invited' \
         FOR UPDATE OF invitation, account",
    )
    .bind(token_digest(token))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("load invitation"))?
    .ok_or(UserAdminError::NotFound)?;
    let invitation_id: i64 = invitation.get(0);
    // The preceding query can wait on row locks. Transaction-start now() is
    // not sufficient to decide whether the invitation is still usable.
    let unexpired: bool = sqlx::query_scalar(
        "SELECT expires_at > clock_timestamp() FROM volund.user_invitations WHERE id=$1",
    )
    .bind(invitation_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("recheck invitation expiry"))?;
    if !unexpired {
        return Err(UserAdminError::NotFound);
    }
    let user_id: i64 = invitation.get(1);
    let public_user_id: String = invitation.get(2);
    let display_name: String = invitation.get(3);
    sqlx::query(
        "INSERT INTO volund.password_credentials (user_id, password_hash, must_change) \
         VALUES ($1, $2, false) ON CONFLICT (user_id) DO UPDATE SET \
         password_hash = EXCLUDED.password_hash, must_change = false, changed_at = now()",
    )
    .bind(user_id)
    .bind(password_hash)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("store invited credential"))?;
    sqlx::query("UPDATE volund.users SET status = 'active', updated_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("activate invited user"))?;
    sqlx::query("UPDATE volund.user_invitations SET accepted_at = now() WHERE id = $1")
        .bind(invitation_id)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("consume invitation"))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id) \
         VALUES ($1, $2::uuid, $3, 'user.invitation_accept', 'success', 'user', $2::uuid)",
    )
    .bind(user_id)
    .bind(public_user_id)
    .bind(display_name)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit invitation acceptance"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit invitation acceptance"))
}

fn validate_token(token: &str) -> Result<(), UserAdminError> {
    if token.len() == TOKEN_BYTES * 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(UserAdminError::NotFound)
    }
}

fn random_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> UserAdminError {
    move |error| UserAdminError::Database(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_tokens_are_random_bounded_and_digestible() {
        let first = random_token();
        let second = random_token();
        assert_eq!(first.len(), TOKEN_BYTES * 2);
        assert_ne!(first, second);
        assert!(validate_token(&first).is_ok());
        assert!(validate_token("short").is_err());
        assert_eq!(token_digest(&first).len(), 64);
    }
}
