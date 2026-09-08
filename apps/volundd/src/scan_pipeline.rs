use sqlx::{PgPool, Row};

use crate::scanner::{self, ScanReport};

#[derive(Debug, Eq, PartialEq)]
pub struct ProcessReport {
    pub id: String,
    pub status: String,
    pub scan: Option<ScanReport>,
}

struct ClaimedScan {
    id: i64,
    public_id: String,
    root_key: String,
    full: bool,
}

/// Claim and execute at most one durable library scan.
///
/// Queued work survives daemon and worker restarts. A run left in `running`
/// for more than two hours is safely requeued because catalog persistence is
/// atomic and the scanner also owns a per-library `PostgreSQL` advisory lock.
///
/// # Errors
///
/// Returns an error when queue recovery or claiming fails. Scan failures are
/// persisted on the run and returned as a successful `failed` report.
pub async fn process_next(pool: &PgPool) -> Result<Option<ProcessReport>, String> {
    let Some(claimed) = claim_next(pool).await? else {
        return Ok(None);
    };
    match scanner::scan_queued(pool, claimed.id, &claimed.root_key, claimed.full).await {
        Ok(scan) => Ok(Some(ProcessReport {
            id: claimed.public_id,
            status: "completed".to_owned(),
            scan: Some(scan),
        })),
        Err(message) => Ok(Some(ProcessReport {
            id: claimed.public_id,
            status: if message == "scan cancellation requested" {
                "cancelled".to_owned()
            } else {
                "failed".to_owned()
            },
            scan: None,
        })),
    }
}

/// Process one bounded parallel batch. # Errors Returns policy, claim, task, or scan failure.
#[allow(clippy::missing_errors_doc)]
pub async fn process_batch(pool: &PgPool) -> Result<Vec<ProcessReport>, String> {
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load scan concurrency".to_owned())?
        .scan_concurrency;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..limit {
        let pool = pool.clone();
        tasks.spawn(async move { process_next(&pool).await });
    }
    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Some(report) =
            result.map_err(|error| format!("scan worker task failed: {error}"))??
        {
            reports.push(report);
        }
    }
    Ok(reports)
}

async fn claim_next(pool: &PgPool) -> Result<Option<ClaimedScan>, String> {
    sqlx::query(
        "UPDATE volund.scan_runs SET \
         status = CASE WHEN cancellation_requested_at IS NULL THEN 'queued' ELSE 'cancelled' END, \
         started_at = now(), finished_at = CASE WHEN cancellation_requested_at IS NULL THEN NULL ELSE now() END, \
         error_message = CASE WHEN cancellation_requested_at IS NULL THEN 'stale scan worker requeued' ELSE NULL END \
         WHERE status = 'running' AND started_at < now() - interval '2 hours'",
    )
    .execute(pool)
    .await
    .map_err(|error| format!("cannot recover stale scan jobs: {error}"))?;
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load scan concurrency".to_owned())?
        .scan_concurrency;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin scan claim: {error}"))?;
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,1)")
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("cannot lock scan claim: {error}"))?;
    let running: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.scan_runs WHERE status='running'")
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("cannot count scan claims: {error}"))?;
    if running >= limit {
        tx.rollback().await.ok();
        return Ok(None);
    }
    let row = sqlx::query(
        "UPDATE volund.scan_runs run SET status = 'running', started_at = now(), \
         finished_at = NULL, error_message = NULL WHERE run.id = ( \
          SELECT queued.id FROM volund.scan_runs queued \
          WHERE queued.status = 'queued' AND queued.cancellation_requested_at IS NULL \
          ORDER BY queued.requested_at, queued.id \
          FOR UPDATE SKIP LOCKED LIMIT 1) \
         RETURNING run.id, run.public_id::text, \
          (SELECT root_key FROM volund.library_roots WHERE id = run.library_root_id), \
          run.full_scan",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("cannot claim scan job: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("cannot commit scan claim: {error}"))?;
    Ok(row.map(|row| ClaimedScan {
        id: row.get(0),
        public_id: row.get(1),
        root_key: row.get(2),
        full: row.get(3),
    }))
}
