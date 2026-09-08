use sqlx::{PgPool, Postgres, Row, Transaction};
use std::path::{Path, PathBuf};
use tokio::fs;

pub(crate) async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    source: i64,
    obsolete: &str,
    keeper: Option<&str>,
    hash: &str,
    size: i64,
) -> Result<(), String> {
    sqlx::query("INSERT INTO volund.source_cleanup (source_file_id,obsolete_relative_path,keeper_relative_path,sha256,byte_size) VALUES ($1,$2,$3,$4,$5)")
        .bind(source).bind(obsolete).bind(keeper).bind(hash).bind(size)
        .execute(&mut **tx).await.map_err(|e| format!("cannot record source cleanup: {e}"))?;
    Ok(())
}

/// Retry committed lifecycle unlinks, including after a daemon restart.
/// # Errors
/// Retains pending intents on unsafe paths, changed bytes or storage/SQL errors.
pub async fn reconcile(pool: &PgPool) -> Result<u64, String> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT source_file_id FROM volund.source_cleanup ORDER BY source_file_id LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut completed = 0;
    let mut failure = None;
    for id in ids {
        match finish(pool, id).await {
            Ok(()) => completed += 1,
            Err(error) => failure = Some(error),
        }
    }
    failure.map_or(Ok(completed), Err)
}

pub(crate) async fn finish(pool: &PgPool, source: i64) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("SELECT id FROM volund.source_files WHERE id=$1 FOR UPDATE")
        .bind(source)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    finish_locked(&mut tx, source).await?;
    tx.commit().await.map_err(|e| e.to_string())
}

// Caller holds the source row lock, shared by all lifecycle transitions.
pub(crate) async fn finish_locked(
    tx: &mut Transaction<'_, Postgres>,
    source: i64,
) -> Result<(), String> {
    let row = sqlx::query("SELECT root.filesystem_path,pending.obsolete_relative_path,pending.keeper_relative_path,pending.sha256,pending.byte_size FROM volund.source_cleanup pending JOIN volund.source_files source ON source.id=pending.source_file_id JOIN volund.library_roots root ON root.id=source.library_root_id WHERE pending.source_file_id=$1")
        .bind(source).fetch_optional(&mut **tx).await.map_err(|e| e.to_string())?;
    let Some(row) = row else {
        return Ok(());
    };
    let root = fs::canonicalize(row.get::<String, _>(0))
        .await
        .map_err(|e| e.to_string())?;
    let obsolete = safe_path(&root, &row.get::<String, _>(1)).await?;
    match fs::symlink_metadata(&obsolete).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.len()
                    != u64::try_from(row.get::<i64, _>(4)).map_err(|e| e.to_string())?
            {
                return Err("cleanup file changed or is not a regular file".to_owned());
            }
            if let Some(keeper) = row.get::<Option<String>, _>(2) {
                let keeper = safe_path(&root, &keeper).await?;
                if !crate::source_link::same_file(&obsolete, &keeper) {
                    return Err("cleanup keeper is missing or no longer the same file".to_owned());
                }
            }
            let hash_path = obsolete.clone();
            let hash =
                tokio::task::spawn_blocking(move || crate::file_hash::sha256_file(&hash_path))
                    .await
                    .map_err(|e| e.to_string())??;
            if hash.as_str() != row.get::<String, _>(3) {
                return Err("cleanup file hash changed".to_owned());
            }
            fs::remove_file(&obsolete)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    crate::source_link::sync_parent(&obsolete).await?;
    sqlx::query("DELETE FROM volund.source_cleanup WHERE source_file_id=$1")
        .bind(source)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = volund_core::normalize_library_path(relative).map_err(str::to_owned)?;
    let path = root.join(relative);
    let parent = fs::canonicalize(path.parent().ok_or("cleanup parent missing")?)
        .await
        .map_err(|e| e.to_string())?;
    if !parent.starts_with(root) {
        return Err("cleanup path escapes library".to_owned());
    }
    Ok(parent.join(path.file_name().ok_or("cleanup filename missing")?))
}
