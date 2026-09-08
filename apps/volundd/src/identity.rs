use std::fs;
use std::path::Path;
use std::sync::Arc;

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand_core::OsRng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use subtle::ConstantTimeEq;

const MIN_PASSWORD_BYTES: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1024;
const MIN_BOOTSTRAP_TOKEN_BYTES: usize = 32;
const MAX_BOOTSTRAP_TOKEN_BYTES: usize = 512;
const BOOTSTRAP_TOKEN_FILE_ENV: &str = "VOLUND_BOOTSTRAP_TOKEN_FILE";

#[derive(Clone, Default)]
pub struct IdentityConfig {
    bootstrap_secret: Option<Arc<BootstrapSecret>>,
}

impl IdentityConfig {
    /// Load the optional first-owner bootstrap secret from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured token file cannot be loaded safely.
    pub fn from_environment() -> Result<Self, IdentityError> {
        let Some(path) = std::env::var_os(BOOTSTRAP_TOKEN_FILE_ENV) else {
            return Ok(Self::default());
        };
        if path.is_empty() {
            return Err(IdentityError::Unavailable(format!(
                "{BOOTSTRAP_TOKEN_FILE_ENV} must not be empty"
            )));
        }
        let secret = BootstrapSecret::from_file(Path::new(&path))?;
        Ok(Self::with_bootstrap_secret(secret))
    }

    #[must_use]
    pub fn with_bootstrap_secret(secret: BootstrapSecret) -> Self {
        Self {
            bootstrap_secret: Some(Arc::new(secret)),
        }
    }

    #[must_use]
    pub fn bootstrap_secret(&self) -> Option<Arc<BootstrapSecret>> {
        self.bootstrap_secret.clone()
    }

    #[must_use]
    pub fn bootstrap_available(&self) -> bool {
        self.bootstrap_secret.is_some()
    }
}

pub struct FirstOwnerInput {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstOwner {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: &'static str,
}

#[derive(Debug, Eq, PartialEq)]
pub enum IdentityError {
    InvalidBootstrapToken,
    AlreadyInitialized,
    InvalidInput(String),
    Unavailable(String),
}

pub struct BootstrapSecret {
    digest: [u8; 32],
}

impl BootstrapSecret {
    /// Load a bootstrap token from an absolute, operator-controlled file.
    ///
    /// # Errors
    ///
    /// Returns an error for a relative path, unreadable file, or invalid token.
    pub fn from_file(path: &Path) -> Result<Self, IdentityError> {
        if !path.is_absolute() {
            return Err(IdentityError::Unavailable(
                "bootstrap token path must be absolute".to_owned(),
            ));
        }
        let token = fs::read_to_string(path).map_err(|error| {
            IdentityError::Unavailable(format!("cannot read bootstrap token file: {error}"))
        })?;
        Self::from_token(token.trim())
    }

    /// Build a secret from a token. This is also used by isolated tests.
    ///
    /// # Errors
    ///
    /// Returns an error when the token is too short, too long, or contains
    /// whitespace.
    pub fn from_token(token: &str) -> Result<Self, IdentityError> {
        validate_bootstrap_token(token)?;
        Ok(Self {
            digest: digest(token.as_bytes()),
        })
    }

    fn verifies(&self, candidate: &str) -> bool {
        self.digest.ct_eq(&digest(candidate.as_bytes())).into()
    }
}

/// Report whether first-owner setup has completed.
///
/// # Errors
///
/// Returns an error when the singleton setup state cannot be read.
pub async fn setup_complete(pool: &PgPool) -> Result<bool, IdentityError> {
    sqlx::query_scalar(
        "SELECT initialized_at IS NOT NULL AND owner_user_id IS NOT NULL \
         FROM volund.instance_state WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .map_err(database_error("read instance setup state"))
}

/// Atomically create the sole first owner of an empty instance.
///
/// # Errors
///
/// Returns an error for an invalid token or input, an initialized instance, or
/// a database/password-hashing failure.
pub async fn create_first_owner(
    pool: &PgPool,
    secret: &BootstrapSecret,
    candidate_token: &str,
    input: FirstOwnerInput,
) -> Result<FirstOwner, IdentityError> {
    if !secret.verifies(candidate_token) {
        record_denied_bootstrap(pool).await?;
        return Err(IdentityError::InvalidBootstrapToken);
    }
    let (email, normalized_email, display_name) = validate_owner(&input)?;
    let password = input.password;
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| {
            IdentityError::Unavailable(format!("password hashing task failed: {error}"))
        })??;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin first-owner setup"))?;
    let initialized: bool = sqlx::query_scalar(
        "SELECT initialized_at IS NOT NULL OR owner_user_id IS NOT NULL \
         FROM volund.instance_state WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("lock instance setup state"))?;
    let user_count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM volund.users")
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error("count existing users"))?;
    if initialized || user_count != 0 {
        return Err(IdentityError::AlreadyInitialized);
    }
    let row = sqlx::query(
        "INSERT INTO volund.users \
         (email, normalized_email, display_name, role, status) \
         VALUES ($1, $2, $3, 'owner', 'active') RETURNING id, public_id::text",
    )
    .bind(&email)
    .bind(&normalized_email)
    .bind(&display_name)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("create first owner"))?;
    let user_id: i64 = row.get(0);
    let public_id: String = row.get(1);
    sqlx::query("INSERT INTO volund.password_credentials (user_id, password_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(password_hash)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("store first-owner credential"))?;
    sqlx::query(
        "UPDATE volund.instance_state SET initialized_at = now(), owner_user_id = $1 \
         WHERE singleton",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("complete instance setup"))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id) \
         VALUES ($1, $2::uuid, $3, 'setup.first_owner', 'success', 'user', $2::uuid)",
    )
    .bind(user_id)
    .bind(&public_id)
    .bind(&display_name)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit first-owner setup"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit first-owner setup"))?;
    Ok(FirstOwner {
        id: public_id,
        email,
        display_name,
        role: "owner",
    })
}

/// Verify a submitted password against a stored PHC string.
#[must_use]
pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    PasswordHash::new(encoded_hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

fn validate_owner(input: &FirstOwnerInput) -> Result<(String, String, String), IdentityError> {
    let email = input.email.trim();
    let normalized_email = email.to_lowercase();
    let valid_email = email.len() <= 320
        && email.len() >= 3
        && !email.chars().any(char::is_whitespace)
        && email.split('@').count() == 2
        && !email.starts_with('@')
        && !email.ends_with('@');
    if !valid_email {
        return Err(IdentityError::InvalidInput("email is invalid".to_owned()));
    }
    let display_name = input.display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 160 {
        return Err(IdentityError::InvalidInput(
            "display name must contain 1 to 160 characters".to_owned(),
        ));
    }
    if !(MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&input.password.len()) {
        return Err(IdentityError::InvalidInput(format!(
            "password must contain {MIN_PASSWORD_BYTES} to {MAX_PASSWORD_BYTES} bytes"
        )));
    }
    Ok((email.to_owned(), normalized_email, display_name.to_owned()))
}

pub(crate) fn hash_password(password: &str) -> Result<String, IdentityError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| IdentityError::Unavailable(format!("cannot hash password: {error}")))
}

fn validate_bootstrap_token(token: &str) -> Result<(), IdentityError> {
    if !(MIN_BOOTSTRAP_TOKEN_BYTES..=MAX_BOOTSTRAP_TOKEN_BYTES).contains(&token.len())
        || token.chars().any(char::is_whitespace)
    {
        return Err(IdentityError::Unavailable(
            "bootstrap token must be 32 to 512 non-whitespace bytes".to_owned(),
        ));
    }
    Ok(())
}

fn digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

async fn record_denied_bootstrap(pool: &PgPool) -> Result<(), IdentityError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events (action, outcome, target_type, metadata) \
         VALUES ('setup.first_owner', 'denied', 'instance', \
         '{\"reason\":\"invalid_bootstrap_token\"}'::jsonb)",
    )
    .execute(pool)
    .await
    .map_err(database_error("audit denied first-owner setup"))?;
    Ok(())
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> IdentityError {
    move |error| IdentityError::Unavailable(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> FirstOwnerInput {
        FirstOwnerInput {
            email: " Owner@Example.test ".to_owned(),
            display_name: " First Owner ".to_owned(),
            password: "correct horse battery staple".to_owned(),
        }
    }

    #[test]
    fn owner_input_is_normalized_and_bounded() {
        let normalized = validate_owner(&valid_input()).expect("valid owner");
        assert_eq!(normalized.0, "Owner@Example.test");
        assert_eq!(normalized.1, "owner@example.test");
        assert_eq!(normalized.2, "First Owner");
        let mut invalid = valid_input();
        invalid.password = "short".to_owned();
        assert!(matches!(
            validate_owner(&invalid),
            Err(IdentityError::InvalidInput(_))
        ));
    }

    #[test]
    fn passwords_are_argon2id_hashes_and_verify() {
        let encoded = hash_password("correct horse battery staple").expect("hash password");
        assert!(encoded.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery staple", &encoded));
        assert!(!verify_password("wrong password", &encoded));
    }

    #[test]
    fn bootstrap_tokens_are_bounded_and_compared() {
        let token = "a".repeat(48);
        let secret = BootstrapSecret::from_token(&token).expect("valid token");
        assert!(secret.verifies(&token));
        assert!(!secret.verifies(&"b".repeat(48)));
        assert!(BootstrapSecret::from_token("too-short").is_err());
    }
}
