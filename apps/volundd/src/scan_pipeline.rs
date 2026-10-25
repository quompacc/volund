use sqlx::{Connection, PgConnection, PgPool, Row};
use tokio_util::sync::CancellationToken;

use crate::job_lease::{self, Lease};
use crate::scanner::{self, ScanReport};

/// Lost or stopped workers tolerated before a scan fails instead of requeueing.
const MAX_SCAN_ATTEMPTS: i32 = 3;
const WORKER_LOST_REQUEUED: &str = "scan worker stopped; scan requeued";
const WORKER_LOST_REPEATEDLY: &str = "scan worker stopped repeatedly; scan failed";

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
/// Queued work survives daemon and worker restarts. The claim is leased to
/// this worker; a run whose worker vanished is requeued by the next claim
/// because catalog persistence is atomic.
///
/// # Errors
///
/// Returns an error when queue recovery or claiming fails. Scan failures are
/// persisted on the run and returned as a successful `failed` report.
pub async fn process_next(pool: &PgPool) -> Result<Option<ProcessReport>, String> {
    process_next_until(pool, &CancellationToken::new()).await
}

/// Like [`process_next`], but an orderly worker stop (`stop`) abandons the
/// uncommitted scan and requeues it.
///
/// # Errors
///
/// Returns an error when queue recovery, claiming, or requeueing fails.
pub async fn process_next_until(
    pool: &PgPool,
    stop: &CancellationToken,
) -> Result<Option<ProcessReport>, String> {
    if stop.is_cancelled() {
        return Ok(None);
    }
    let Some((claimed, mut lease)) = claim_next(pool).await? else {
        return Ok(None);
    };
    let result = tokio::select! {
        result = scanner::scan_queued(pool, claimed.id, &claimed.root_key, claimed.full) => result,
        () = stop.cancelled() => {
            let status = requeue(lease.connection(), claimed.id).await?;
            lease.release().await;
            return Ok(Some(ProcessReport { id: claimed.public_id, status, scan: None }));
        }
    };
    lease.release().await;
    match result {
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
pub async fn process_batch(
    pool: &PgPool,
    stop: &CancellationToken,
) -> Result<Vec<ProcessReport>, String> {
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load scan concurrency".to_owned())?
        .scan_concurrency;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..limit {
        let pool = pool.clone();
        let stop = stop.clone();
        tasks.spawn(async move { process_next_until(&pool, &stop).await });
    }
    let mut reports = Vec::new();
    let mut first_error = None;
    // Let sibling scans finish; aborting them would orphan their claims.
    while let Some(result) = tasks.join_next().await {
        match result.map_err(|error| format!("scan worker task failed: {error}")) {
            Ok(Ok(Some(report))) => reports.push(report),
            Ok(Ok(None)) => {}
            Ok(Err(error)) | Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    first_error.map_or(Ok(reports), Err)
}

/// Requeue every `running` scan that neither a live worker lease nor a direct
/// CLI scan (the per-library lock) still owns.
///
/// Persistence is one transaction, so a lost scan left no partial catalog
/// state. After `MAX_SCAN_ATTEMPTS` lost workers the scan fails instead of
/// requeueing forever; a pending cancellation always wins.
pub(crate) async fn recover_orphans(pool: &PgPool) -> Result<u64, String> {
    let mut sweep = Lease::open(pool).await?;
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT id, library_root_id FROM volund.scan_runs WHERE status='running' ORDER BY id",
    )
    .fetch_all(sweep.connection())
    .await
    .map_err(|error| format!("cannot list running scans: {error}"))?;
    let mut recovered = 0;
    for (id, root) in rows {
        if !job_lease::try_hold(sweep.connection(), job_lease::SCAN, id).await? {
            continue;
        }
        let library_free: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1 + $2)")
            .bind(scanner::SCAN_LOCK_NAMESPACE)
            .bind(root)
            .fetch_one(sweep.connection())
            .await
            .map_err(|error| format!("cannot inspect scan lock: {error}"))?;
        if library_free {
            // Release the probe at once: a claimer of the requeued scan needs it.
            sqlx::query("SELECT pg_advisory_unlock($1 + $2)")
                .bind(scanner::SCAN_LOCK_NAMESPACE)
                .bind(root)
                .execute(sweep.connection())
                .await
                .map_err(|error| format!("cannot release scan lock probe: {error}"))?;
            if requeue(sweep.connection(), id).await? != "lost" {
                recovered += 1;
            }
        }
        job_lease::drop_hold(sweep.connection(), job_lease::SCAN, id).await?;
    }
    sweep.release().await;
    Ok(recovered)
}

/// Requeue a scan whose worker is gone or stopping, or end it once the attempt
/// cap is reached or cancellation was requested. Returns the new status.
async fn requeue(connection: &mut PgConnection, id: i64) -> Result<String, String> {
    sqlx::query_scalar(
        "UPDATE volund.scan_runs SET \
         status = CASE WHEN cancellation_requested_at IS NOT NULL THEN 'cancelled' \
          WHEN attempt >= $2 THEN 'failed' ELSE 'queued' END, \
         attempt = CASE WHEN cancellation_requested_at IS NULL AND attempt < $2 \
          THEN attempt + 1 ELSE attempt END, \
         finished_at = CASE WHEN cancellation_requested_at IS NULL AND attempt < $2 \
          THEN NULL ELSE now() END, \
         error_message = CASE WHEN cancellation_requested_at IS NOT NULL THEN NULL \
          WHEN attempt >= $2 THEN $3 ELSE $4 END \
         WHERE id = $1 AND status = 'running' RETURNING status",
    )
    .bind(id)
    .bind(MAX_SCAN_ATTEMPTS)
    .bind(WORKER_LOST_REPEATEDLY)
    .bind(WORKER_LOST_REQUEUED)
    .fetch_optional(connection)
    .await
    .map_err(|error| format!("cannot requeue scan: {error}"))
    .map(|status| status.unwrap_or_else(|| "lost".to_owned()))
}

async fn claim_next(pool: &PgPool) -> Result<Option<(ClaimedScan, Lease)>, String> {
    recover_orphans(pool).await?;
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load scan concurrency".to_owned())?
        .scan_concurrency;
    let mut lease = Lease::open(pool).await?;
    let mut tx = lease
        .connection()
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
        lease.release().await;
        return Ok(None);
    }
    // A requeue diagnostic stays visible while the scan runs again.
    let row = sqlx::query(
        "UPDATE volund.scan_runs run SET status = 'running', started_at = now(), \
         finished_at = NULL WHERE run.id = ( \
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
    let Some(row) = row else {
        tx.rollback().await.ok();
        lease.release().await;
        return Ok(None);
    };
    let claimed = ClaimedScan {
        id: row.get(0),
        public_id: row.get(1),
        root_key: row.get(2),
        full: row.get(3),
    };
    job_lease::hold(&mut tx, job_lease::SCAN, claimed.id).await?;
    tx.commit()
        .await
        .map_err(|error| format!("cannot commit scan claim: {error}"))?;
    Ok(Some((claimed, lease)))
}
