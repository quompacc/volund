use sqlx::{PgPool, Postgres, Transaction};

pub(crate) async fn cancel_before_persist(
    transaction: &mut Transaction<'_, Postgres>,
    scan_id: i64,
) -> Result<bool, String> {
    let cancelled: bool = sqlx::query_scalar(
        "SELECT cancellation_requested_at IS NOT NULL FROM volund.scan_runs WHERE id = $1 FOR UPDATE",
    )
    .bind(scan_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| format!("cannot lock scan completion: {error}"))?;
    if cancelled {
        sqlx::query(
            "UPDATE volund.scan_runs SET status = 'cancelled', finished_at = now(), error_message = NULL WHERE id = $1",
        )
        .bind(scan_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| format!("cannot complete scan cancellation: {error}"))?;
    }
    Ok(cancelled)
}

pub(crate) async fn mark_cancelled(pool: &PgPool, scan_id: i64) {
    let _ = sqlx::query(
        "UPDATE volund.scan_runs SET status = 'cancelled', finished_at = now(), error_message = NULL \
         WHERE id = $1 AND status = 'running' AND cancellation_requested_at IS NOT NULL",
    )
    .bind(scan_id)
    .execute(pool)
    .await;
}

pub(crate) async fn ensure_not_cancelled(pool: &PgPool, scan_id: i64) -> Result<(), String> {
    let cancelled: bool = sqlx::query_scalar(
        "SELECT cancellation_requested_at IS NOT NULL FROM volund.scan_runs WHERE id = $1",
    )
    .bind(scan_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot inspect scan cancellation: {error}"))?;
    if cancelled {
        Err("scan cancellation requested".to_owned())
    } else {
        Ok(())
    }
}

pub(crate) async fn update_progress(
    pool: &PgPool,
    scan_id: i64,
    discovered: usize,
    hashed: usize,
) -> Result<(), String> {
    let discovered = i64::try_from(discovered).map_err(|_| "scan progress overflow".to_owned())?;
    let hashed = i64::try_from(hashed).map_err(|_| "scan progress overflow".to_owned())?;
    sqlx::query(
        "UPDATE volund.scan_runs SET discovered_files = $2, hashed_files = $3 WHERE id = $1 AND status = 'running'",
    )
    .bind(scan_id)
    .bind(discovered)
    .bind(hashed)
    .execute(pool)
    .await
    .map_err(|error| format!("cannot update scan progress: {error}"))?;
    Ok(())
}
