use sqlx::{PgPool, Row};

use crate::api_models::{ImportDraftItemSummary, ImportDraftSummary};
use crate::import_review::ImportReviewError;
use crate::session::AuthenticatedSession;

/// Load the newest fully staged draft owned by the actor.
///
/// # Errors
/// Returns a database diagnostic when the resumable draft cannot be queried.
pub async fn latest_uploaded(
    pool: &PgPool,
    actor: &AuthenticatedSession,
) -> Result<Option<ImportDraftSummary>, ImportReviewError> {
    let row = sqlx::query(
        "SELECT d.id,d.public_id::text,d.source_name,d.suggested_model_name,d.suggested_slug, \
         d.total_files::bigint,d.total_bytes FROM volund.import_drafts d \
         WHERE d.status IN ('uploaded','review_ready','reviewed','failed') \
         AND d.owner_user_id=$1 AND d.configured_at IS NOT NULL AND d.expires_at>now() \
         AND NOT EXISTS (SELECT 1 FROM volund.import_draft_items i \
         WHERE i.import_draft_id=d.id AND i.upload_status<>'uploaded') \
         ORDER BY d.created_at DESC LIMIT 1",
    )
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database_error("load latest import"))?;
    match row {
        Some(row) => build_summary(pool, &row).await.map(Some),
        None => Ok(None),
    }
}

/// Load the persisted manifest for one active draft owned by the actor.
///
/// # Errors
/// Returns an ownership-safe not-found response or a database diagnostic.
pub async fn draft_manifest(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
) -> Result<ImportDraftSummary, ImportReviewError> {
    let row = sqlx::query(
        "SELECT d.id,d.public_id::text,d.source_name,d.suggested_model_name,d.suggested_slug, \
         d.total_files::bigint,d.total_bytes FROM volund.import_drafts d \
         WHERE d.public_id::text=$1 AND d.owner_user_id=$2 AND d.expires_at>now() \
         AND d.status IN ('draft','uploading','uploaded','review_ready','reviewed','failed')",
    )
    .bind(draft_id)
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database_error("load import manifest"))?
    .ok_or_else(|| ImportReviewError::NotFound("unknown import draft".to_owned()))?;
    build_summary(pool, &row).await
}

async fn build_summary(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
) -> Result<ImportDraftSummary, ImportReviewError> {
    let items = sqlx::query(
        "SELECT public_id::text,original_path,byte_size,category,suggested_relative_path, \
         is_primary_candidate,upload_status FROM volund.import_draft_items \
         WHERE import_draft_id=$1 ORDER BY id",
    )
    .bind(row.get::<i64, _>(0))
    .fetch_all(pool)
    .await
    .map_err(database_error("load import items"))?
    .into_iter()
    .map(|item| ImportDraftItemSummary {
        id: item.get(0),
        original_path: item.get(1),
        byte_size: item.get(2),
        category: item.get(3),
        suggested_relative_path: item.get(4),
        is_primary_candidate: item.get(5),
        upload_status: item.get(6),
    })
    .collect();
    Ok(ImportDraftSummary {
        id: row.get(1),
        source_name: row.get(2),
        suggested_model_name: row.get(3),
        suggested_slug: row.get(4),
        total_files: row.get(5),
        total_bytes: row.get(6),
        items,
    })
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> ImportReviewError {
    move |error| ImportReviewError::Database(format!("cannot {context}: {error}"))
}
