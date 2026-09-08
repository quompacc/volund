use std::path::Path;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use tokio::fs;

use crate::api_models::{
    ImportCancelSummary, ImportDraftLifecycleSummary, ImportDraftPage, ImportStorageSummary,
};
use crate::session::AuthenticatedSession;

#[derive(Debug)]
pub enum ImportLifecycleError {
    BadRequest(String),
    NotFound,
    Conflict(String),
    Storage(String),
    Database(String),
}

/// List bounded import drafts visible to the actor.
///
/// # Errors
/// Returns validation or database errors without disclosing foreign drafts.
pub async fn list(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<ImportDraftPage, ImportLifecycleError> {
    if !(1..=100).contains(&limit) || offset < 0 {
        return Err(ImportLifecycleError::BadRequest(
            "limit must be 1..100 and offset must not be negative".to_owned(),
        ));
    }
    if status.is_some_and(|value| !valid_status(value)) {
        return Err(ImportLifecycleError::BadRequest(
            "unknown import status filter".to_owned(),
        ));
    }
    let privileged = matches!(actor.role.as_str(), "owner" | "administrator");
    let rows = sqlx::query(
        "SELECT d.public_id::text,d.display_name,d.status,u.public_id::text,u.display_name, \
         d.total_files::bigint,count(i.id) FILTER (WHERE i.upload_status='uploaded')::bigint, \
         d.total_bytes,d.uploaded_bytes,d.created_at,d.updated_at,d.expires_at,d.target_action, \
         model.public_id::text,d.last_error_code,d.result_model_public_id::text, \
         count(*) OVER()::bigint FROM volund.import_drafts d \
         LEFT JOIN volund.users u ON u.id=d.owner_user_id \
         LEFT JOIN volund.models model ON model.id=d.target_model_id \
         LEFT JOIN volund.import_draft_items i ON i.import_draft_id=d.id \
         WHERE ($1 OR d.owner_user_id=$2) AND ($3::text IS NULL OR d.status=$3) \
         GROUP BY d.id,u.public_id,u.display_name,model.public_id \
         ORDER BY d.updated_at DESC,d.id DESC LIMIT $4 OFFSET $5",
    )
    .bind(privileged)
    .bind(actor.database_user_id())
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(database_error("list import drafts"))?;
    let total = rows.first().map_or(0, |row| row.get(16));
    let items = rows.iter().map(row_summary).collect();
    Ok(ImportDraftPage {
        items,
        limit,
        offset,
        total,
    })
}

/// Rename one owned draft without changing any staging or target path.
///
/// # Errors
/// Returns validation, ownership-safe not-found, or database errors.
pub async fn rename(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
    display_name: &str,
) -> Result<ImportDraftLifecycleSummary, ImportLifecycleError> {
    let display_name = display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 160 {
        return Err(ImportLifecycleError::BadRequest(
            "displayName must contain between 1 and 160 characters".to_owned(),
        ));
    }
    let privileged = matches!(actor.role.as_str(), "owner" | "administrator");
    let row = sqlx::query(
        "UPDATE volund.import_drafts SET display_name=$3,updated_at=now() \
         WHERE public_id::text=$1 AND ($2 OR owner_user_id=$4) \
         AND status NOT IN ('committed','cancelled','expired','committing') RETURNING id",
    )
    .bind(draft_id)
    .bind(privileged)
    .bind(display_name)
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database_error("rename import draft"))?;
    if row.is_none() {
        return Err(ImportLifecycleError::NotFound);
    }
    load_one(pool, actor, draft_id).await
}

/// Return exact persisted staging accounting, separated by lifecycle meaning.
///
/// # Errors
/// Returns a database error when accounting cannot be loaded.
pub async fn storage(pool: &PgPool) -> Result<ImportStorageSummary, ImportLifecycleError> {
    let policy = crate::import_policy::load(pool).await.map_err(|error| {
        ImportLifecycleError::Database(format!("cannot load import policy: {error:?}"))
    })?;
    let row = sqlx::query(
        "SELECT COALESCE(sum(total_bytes) FILTER (WHERE status IN \
         ('draft','uploading','uploaded','review_ready','reviewed','failed')),0)::bigint, \
         COALESCE(sum(uploaded_bytes) FILTER (WHERE status IN \
         ('draft','uploading','uploaded','review_ready','reviewed','failed','committing') OR \
         (status='committed' AND staging_cleaned_at IS NULL)),0)::bigint, \
         COALESCE(sum(uploaded_bytes) FILTER (WHERE status IN ('cancelled','expired','committed') AND staging_cleaned_at IS NULL),0)::bigint \
         FROM volund.import_drafts",
    )
    .fetch_one(pool)
    .await
    .map_err(database_error("inspect import storage"))?;
    Ok(ImportStorageSummary {
        reserved_bytes: row.get(0),
        uploaded_bytes: row.get(1),
        reclaimable_bytes: row.get(2),
        capacity_bytes: policy.incoming_capacity_bytes,
    })
}

/// Cancel one draft and remove only its UUID staging directory.
///
/// # Errors
/// Returns confirmation, ownership, state, storage, or database errors.
pub async fn cancel(
    pool: &PgPool,
    incoming_root: &Path,
    actor: &AuthenticatedSession,
    draft_id: &str,
    confirmation: &str,
) -> Result<ImportCancelSummary, ImportLifecycleError> {
    if confirmation != format!("CANCEL IMPORT {draft_id}") {
        crate::security_audit::denied_target(
            pool,
            actor,
            "import.cancel",
            "import-draft",
            draft_id,
            "confirmation_mismatch",
        )
        .await;
        return Err(ImportLifecycleError::BadRequest(
            "exact draft-bound confirmation is required".to_owned(),
        ));
    }
    let privileged = matches!(actor.role.as_str(), "owner" | "administrator");
    let mut tx = pool
        .begin()
        .await
        .map_err(database_error("begin import cancellation"))?;
    let row = sqlx::query(
        "SELECT id,status,owner_user_id,staging_cleaned_at IS NOT NULL FROM volund.import_drafts \
         WHERE public_id::text=$1 FOR UPDATE",
    )
    .bind(draft_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error("lock import draft"))?
    .ok_or(ImportLifecycleError::NotFound)?;
    if !privileged && row.get::<Option<i64>, _>(2) != Some(actor.database_user_id()) {
        return Err(ImportLifecycleError::NotFound);
    }
    let status: String = row.get(1);
    if status == "cancelled" {
        tx.rollback().await.ok();
        if !row.get::<bool, _>(3) {
            remove_staging(incoming_root, draft_id).await?;
            sqlx::query("UPDATE volund.import_drafts SET staging_cleaned_at=now(),uploaded_bytes=0,updated_at=now() WHERE id=$1")
                .bind(row.get::<i64,_>(0)).execute(pool).await.map_err(database_error("complete import cancellation"))?;
        }
        return Ok(ImportCancelSummary {
            id: draft_id.to_owned(),
            status,
            staging_cleaned: true,
        });
    }
    if matches!(status.as_str(), "committing" | "committed") {
        return Err(ImportLifecycleError::Conflict(
            "an import already committing cannot be cancelled".to_owned(),
        ));
    }
    sqlx::query(
        "UPDATE volund.import_drafts SET status='cancelled',cancelled_at=now(), \
         updated_at=now(),last_error_code=NULL WHERE id=$1",
    )
    .bind(row.get::<i64, _>(0))
    .execute(&mut *tx)
    .await
    .map_err(database_error("cancel import draft"))?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "import.cancel",
        "import-draft",
        draft_id,
        serde_json::json!({"previousStatus":status}),
    )
    .await
    .map_err(database_error("audit import cancellation"))?;
    tx.commit()
        .await
        .map_err(database_error("commit import cancellation"))?;
    remove_staging(incoming_root, draft_id).await?;
    sqlx::query(
        "UPDATE volund.import_drafts SET staging_cleaned_at=now(),uploaded_bytes=0,updated_at=now() \
         WHERE public_id::text=$1 AND status='cancelled'",
    )
    .bind(draft_id)
    .execute(pool)
    .await
    .map_err(database_error("complete import cancellation"))?;
    Ok(ImportCancelSummary {
        id: draft_id.to_owned(),
        status: "cancelled".to_owned(),
        staging_cleaned: true,
    })
}

/// Load one visible draft lifecycle summary.
///
/// # Errors
/// Returns ownership-safe not-found or database errors.
pub async fn load_one(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
) -> Result<ImportDraftLifecycleSummary, ImportLifecycleError> {
    let privileged = matches!(actor.role.as_str(), "owner" | "administrator");
    let row = sqlx::query(
        "SELECT d.public_id::text,d.display_name,d.status,u.public_id::text,u.display_name, \
         d.total_files::bigint,count(i.id) FILTER (WHERE i.upload_status='uploaded')::bigint, \
         d.total_bytes,d.uploaded_bytes,d.created_at,d.updated_at,d.expires_at,d.target_action, \
         model.public_id::text,d.last_error_code,d.result_model_public_id::text \
         FROM volund.import_drafts d LEFT JOIN volund.users u ON u.id=d.owner_user_id \
         LEFT JOIN volund.models model ON model.id=d.target_model_id \
         LEFT JOIN volund.import_draft_items i ON i.import_draft_id=d.id \
         WHERE d.public_id::text=$1 AND ($2 OR d.owner_user_id=$3) \
         GROUP BY d.id,u.public_id,u.display_name,model.public_id",
    )
    .bind(draft_id)
    .bind(privileged)
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database_error("load import draft"))?
    .ok_or(ImportLifecycleError::NotFound)?;
    Ok(row_summary(&row))
}

pub(crate) async fn remove_staging(
    root: &Path,
    draft_id: &str,
) -> Result<(), ImportLifecycleError> {
    if draft_id.len() != 36
        || !draft_id
            .bytes()
            .all(|value| value.is_ascii_hexdigit() || value == b'-')
    {
        return Err(ImportLifecycleError::BadRequest(
            "invalid draft ID".to_owned(),
        ));
    }
    let root = fs::canonicalize(root)
        .await
        .map_err(storage_error("resolve incoming root"))?;
    let target = root.join(draft_id);
    match fs::symlink_metadata(&target).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            ImportLifecycleError::Storage("unsafe import staging entry".to_owned()),
        ),
        Ok(_) => fs::remove_dir_all(&target)
            .await
            .map_err(storage_error("remove import staging")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error("inspect import staging")(error)),
    }
}

fn row_summary(row: &sqlx::postgres::PgRow) -> ImportDraftLifecycleSummary {
    let status: String = row.get(2);
    ImportDraftLifecycleSummary {
        id: row.get(0),
        display_name: row.get(1),
        status: status.clone(),
        actor_id: row.get(3),
        actor_name: row.get(4),
        total_files: row.get(5),
        uploaded_files: row.get(6),
        total_bytes: row.get(7),
        uploaded_bytes: row.get(8),
        created_at_unix_ms: row.get::<DateTime<Utc>, _>(9).timestamp_millis(),
        updated_at_unix_ms: row.get::<DateTime<Utc>, _>(10).timestamp_millis(),
        expires_at_unix_ms: row.get::<DateTime<Utc>, _>(11).timestamp_millis(),
        target_action: row.get(12),
        target_model_id: row.get(13),
        last_error_code: row.get(14),
        result_model_id: row.get(15),
        can_retry: matches!(status.as_str(), "failed" | "reviewed"),
        can_cancel: !matches!(
            status.as_str(),
            "committing" | "committed" | "cancelled" | "expired"
        ),
    }
}

fn valid_status(value: &str) -> bool {
    matches!(
        value,
        "draft"
            | "uploading"
            | "uploaded"
            | "review_ready"
            | "reviewed"
            | "committing"
            | "committed"
            | "failed"
            | "cancelled"
            | "expired"
    )
}
fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> ImportLifecycleError {
    move |error| ImportLifecycleError::Database(format!("cannot {context}: {error}"))
}
fn storage_error(context: &'static str) -> impl FnOnce(std::io::Error) -> ImportLifecycleError {
    move |error| ImportLifecycleError::Storage(format!("cannot {context}: {error}"))
}
