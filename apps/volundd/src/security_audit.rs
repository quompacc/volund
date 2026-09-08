use serde_json::json;
use sqlx::PgPool;

use crate::session::AuthenticatedSession;

/// Record a denied critical transition without submitted confirmation text.
pub async fn denied(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    action: &str,
    target_type: &str,
    code: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,metadata) \
         VALUES ($1,$2::uuid,$3,$4,'denied',$5,$6)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(target_type)
    .bind(json!({"code": code}))
    .execute(pool)
    .await;
}

/// Record a denied critical transition against a stable target ID.
pub async fn denied_target(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    action: &str,
    target_type: &str,
    target_id: &str,
    code: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) \
         VALUES ($1,$2::uuid,$3,$4,'denied',$5,$6::uuid,$7)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(json!({"code": code}))
    .execute(pool)
    .await;
}
