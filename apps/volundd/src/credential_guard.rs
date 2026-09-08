use sqlx::{Postgres, Transaction};
use std::sync::OnceLock;

// All credential writers and session creators lock the user before the credential.
// Read the credential in a separate statement AFTER acquiring the user lock:
// a snapshot taken before waiting must not authorize an obsolete password.
pub(crate) async fn lock_current(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    expected_hash: &str,
) -> Result<bool, sqlx::Error> {
    let usable: Option<bool> = sqlx::query_scalar(
        "SELECT status='active' AND (locked_until IS NULL OR locked_until<=now())
         FROM volund.users WHERE id=$1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    let current: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM volund.password_credentials WHERE user_id=$1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(usable == Some(true) && current.as_deref() == Some(expected_hash))
}

pub(crate) fn dummy_password_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        use argon2::Argon2;
        use argon2::password_hash::{PasswordHasher, SaltString};
        let salt = SaltString::generate(&mut rand_core::OsRng);
        Argon2::default()
            .hash_password(b"dummy password used only for timing", &salt)
            .expect("static dummy password is hashable")
            .to_string()
    })
    .clone()
}
