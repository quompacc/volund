use serde_json::Value;
use sqlx::{Postgres, Transaction};

use crate::session::AuthenticatedSession;

/// Append bounded actor evidence inside the same catalog transaction.
///
/// # Errors
/// Returns a database error when the append-only event cannot be stored.
pub async fn record(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    target_type: &str,
    target_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) \
         VALUES ($1,$2::uuid,$3,$4,'success',$5,$6::uuid,$7)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
