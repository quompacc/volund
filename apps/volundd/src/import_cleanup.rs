use std::path::Path;

use serde_json::json;
use sqlx::{PgPool, Row};

use crate::import_lifecycle::remove_staging;

#[derive(Debug)]
pub struct CleanupResult {
    pub id: String,
    pub status: String,
    pub expired_drafts: i64,
    pub cleaned_drafts: i64,
    pub cleaned_bytes: i64,
}

/// Expire inactive drafts and safely remove terminal staging directories.
///
/// # Errors
/// Returns a sanitized failure when database or staging cleanup cannot proceed.
pub async fn run(pool: &PgPool, incoming_root: &Path) -> Result<CleanupResult, String> {
    let expired = expire_candidates(pool).await?;
    let candidates = sqlx::query(
        "SELECT public_id::text,uploaded_bytes FROM volund.import_drafts \
         WHERE status IN ('cancelled','expired','committed') AND staging_cleaned_at IS NULL \
         ORDER BY updated_at,id LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot load cleanup candidates: {error}"))?;
    let mut cleaned = 0_i64;
    let mut bytes = 0_i64;
    let mut codes = Vec::new();
    for row in candidates {
        let id: String = row.get(0);
        match finish_staging(pool, incoming_root, &id).await {
            Ok(Some(staged_bytes)) => {
                cleaned += 1;
                bytes = bytes.saturating_add(staged_bytes);
            }
            Ok(None) => {}
            Err(_) => codes.push("staging_cleanup_refused"),
        }
    }
    let status = if codes.is_empty() {
        "completed"
    } else {
        "partial"
    };
    let id: String = sqlx::query_scalar(
        "INSERT INTO volund.import_cleanup_runs \
         (status,expired_drafts,cleaned_drafts,cleaned_bytes,result_codes) \
         VALUES ($1,$2,$3,$4,$5) RETURNING public_id::text",
    )
    .bind(status)
    .bind(expired)
    .bind(cleaned)
    .bind(bytes)
    .bind(json!(codes))
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot record import cleanup: {error}"))?;
    Ok(CleanupResult {
        id,
        status: status.to_owned(),
        expired_drafts: expired,
        cleaned_drafts: cleaned,
        cleaned_bytes: bytes,
    })
}

async fn expire_candidates(pool: &PgPool) -> Result<i64, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin import expiry: {error}"))?;
    let rows = sqlx::query(
        "SELECT id FROM volund.import_drafts WHERE expires_at<=now() AND status IN \
         ('draft','uploading','uploaded','review_ready','reviewed','failed') \
         ORDER BY expires_at,id FOR UPDATE SKIP LOCKED LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|error| format!("cannot lock expired imports: {error}"))?;
    let ids: Vec<i64> = rows.into_iter().map(|row| row.get(0)).collect();
    let count = i64::try_from(ids.len()).map_err(|_| "too many expired imports".to_owned())?;
    if !ids.is_empty() {
        sqlx::query(
            "UPDATE volund.import_drafts SET status='expired',expired_at=now(),updated_at=now(), \
             last_error_code='draft_expired' WHERE id=ANY($1)",
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("cannot expire imports: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("cannot commit import expiry: {error}"))?;
    Ok(count)
}

pub(crate) async fn finish_staging(
    pool: &PgPool,
    root: &Path,
    id: &str,
) -> Result<Option<i64>, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let bytes: Option<i64> = sqlx::query_scalar(
        "SELECT uploaded_bytes FROM volund.import_drafts WHERE public_id::text=$1 AND status IN ('cancelled','expired','committed') AND staging_cleaned_at IS NULL FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await.map_err(|e| e.to_string())?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if remove_staging(root, id).await.is_err() {
        sqlx::query("UPDATE volund.import_drafts SET last_error_code='staging_cleanup_refused' WHERE public_id::text=$1")
            .bind(id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        return Err("staging cleanup refused; retry after restoring storage access".to_owned());
    }
    sqlx::query("UPDATE volund.import_drafts SET staging_cleaned_at=now(),uploaded_bytes=0,updated_at=now(),last_error_code=CASE WHEN last_error_code='staging_cleanup_refused' THEN NULL ELSE last_error_code END WHERE public_id::text=$1")
        .bind(id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(Some(bytes))
}
