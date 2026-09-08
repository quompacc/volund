use crate::import_review::ImportReviewError;
use crate::session::AuthenticatedSession;
use sqlx::PgPool;
use volund_core::normalize_library_path;

/// Persist an explicit conflict decision without requiring another upload.
///
/// # Errors
/// Returns validation, ownership-safe not-found, or database errors.
pub async fn resolve_item(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
    item_id: &str,
    action: &str,
    target_path: Option<&str>,
) -> Result<(), ImportReviewError> {
    if !matches!(action, "create" | "reuse" | "relocate" | "skip") {
        return Err(ImportReviewError::BadRequest(
            "unsupported conflict resolution".to_owned(),
        ));
    }
    let normalized_target = target_path
        .map(normalize_library_path)
        .transpose()
        .map_err(|message| ImportReviewError::BadRequest(message.to_owned()))?;
    if action == "create" && normalized_target.is_none() {
        return Err(ImportReviewError::BadRequest(
            "create resolution requires targetPath".to_owned(),
        ));
    }
    if normalized_target
        .as_ref()
        .is_some_and(|path| std::path::Path::new(path).starts_with(".volund-quarantine"))
    {
        return Err(ImportReviewError::BadRequest(
            "the internal quarantine directory is not an import destination".to_owned(),
        ));
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ImportReviewError::Database(e.to_string()))?;
    let protected: Option<bool> = sqlx::query_scalar(
        "SELECT COALESCE(i.id=d.primary_item_id OR i.id=d.thumbnail_item_id,false) \
         FROM volund.import_drafts d JOIN volund.import_draft_items i ON i.import_draft_id=d.id \
         WHERE d.public_id::text=$1 AND d.owner_user_id=$2 AND i.public_id::text=$3 \
         AND d.status IN ('reviewed','failed') FOR UPDATE OF d,i",
    )
    .bind(draft_id)
    .bind(actor.database_user_id())
    .bind(item_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ImportReviewError::Database(e.to_string()))?;
    let protected =
        protected.ok_or_else(|| ImportReviewError::NotFound("unknown review item".to_owned()))?;
    if action == "skip" && protected {
        return Err(ImportReviewError::BadRequest(
            "a selected primary or thumbnail item cannot be skipped; change the selection first"
                .to_owned(),
        ));
    }
    let updated = sqlx::query(
        "UPDATE volund.import_draft_items i SET resolution=$4,planned_action=$4, \
         resolution_relative_path=$5,planned_relative_path=COALESCE($5,planned_relative_path) \
         FROM volund.import_drafts d WHERE d.id=i.import_draft_id AND d.public_id::text=$1 \
         AND d.owner_user_id=$2 AND i.public_id::text=$3 AND d.status IN ('reviewed','failed')",
    )
    .bind(draft_id)
    .bind(actor.database_user_id())
    .bind(item_id)
    .bind(action)
    .bind(normalized_target)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        ImportReviewError::Database(format!("cannot resolve import conflict: {error}"))
    })?;
    if updated.rows_affected() == 0 {
        return Err(ImportReviewError::NotFound(
            "unknown review item".to_owned(),
        ));
    }
    tx.commit()
        .await
        .map_err(|e| ImportReviewError::Database(e.to_string()))?;
    Ok(())
}
