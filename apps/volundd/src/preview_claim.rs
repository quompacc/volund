//! Leased claiming and orphan recovery for preview conversion runs.

use serde_json::json;
use sqlx::{Connection, PgPool, Row};

use crate::job_lease::{self, Lease};

/// Diagnostic recorded on a conversion whose worker vanished mid-run.
pub(crate) const WORKER_LOST: &str = "conversion worker stopped before completion";

pub(crate) struct ClaimedRun {
    pub(crate) id: i64,
    pub(crate) public_id: String,
    pub(crate) content_id: i64,
    pub(crate) sha256: String,
    pub(crate) profile: String,
    pub(crate) linear_deflection: Option<f64>,
    pub(crate) angular_deflection: Option<f64>,
}

/// Finish every `running` conversion whose lease no live worker holds.
///
/// A run with a pending cancellation becomes `cancelled`; any other run becomes
/// `failed` and keeps the explicit, audited retry as its only way forward.
pub(crate) async fn recover_orphans(pool: &PgPool) -> Result<u64, String> {
    let mut sweep = Lease::open(pool).await?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM volund.conversion_runs WHERE status='running' ORDER BY id",
    )
    .fetch_all(sweep.connection())
    .await
    .map_err(|error| format!("cannot list running preview jobs: {error}"))?;
    let diagnostics = json!([{ "message": WORKER_LOST }]).to_string();
    let mut recovered = 0;
    for id in ids {
        if !job_lease::try_hold(sweep.connection(), job_lease::CONVERSION, id).await? {
            continue;
        }
        recovered += sqlx::query(
            "UPDATE volund.conversion_runs SET \
             status = CASE WHEN cancellation_requested_at IS NULL THEN 'failed' ELSE 'cancelled' END, \
             finished_at = now(), diagnostics = CASE WHEN cancellation_requested_at IS NULL \
             THEN $2::jsonb ELSE '[]'::jsonb END WHERE id = $1 AND status = 'running'",
        )
        .bind(id)
        .bind(&diagnostics)
        .execute(sweep.connection())
        .await
        .map_err(|error| format!("cannot recover orphaned preview job: {error}"))?
        .rows_affected();
        job_lease::drop_hold(sweep.connection(), job_lease::CONVERSION, id).await?;
    }
    sweep.release().await;
    Ok(recovered)
}

/// Claim the oldest queued conversion and return it with the held lease.
pub(crate) async fn claim_next(pool: &PgPool) -> Result<Option<(ClaimedRun, Lease)>, String> {
    recover_orphans(pool).await?;
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load conversion concurrency".to_owned())?
        .conversion_concurrency;
    let mut lease = Lease::open(pool).await?;
    let mut tx = lease
        .connection()
        .begin()
        .await
        .map_err(|error| format!("cannot begin preview claim: {error}"))?;
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,2)")
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("cannot lock preview claim: {error}"))?;
    let running: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.conversion_runs WHERE status='running'")
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("cannot count preview claims: {error}"))?;
    if running >= limit {
        tx.rollback().await.ok();
        lease.release().await;
        return Ok(None);
    }
    let row = sqlx::query(
        "UPDATE volund.conversion_runs run SET status = 'running', started_at = now(), \
         converter_version = $1 WHERE run.id = ( \
          SELECT queued.id FROM volund.conversion_runs queued \
          WHERE queued.status = 'queued' AND queued.cancellation_requested_at IS NULL \
          ORDER BY queued.requested_at, queued.id \
          FOR UPDATE SKIP LOCKED LIMIT 1) \
         RETURNING run.id, run.public_id::text, run.content_object_id, \
          (SELECT sha256 FROM volund.content_objects WHERE id = run.content_object_id), run.profile, \
          (run.profile_snapshot->>'linearDeflection')::double precision, \
          (run.profile_snapshot->>'angularDeflection')::double precision",
    )
    .bind(env!("CARGO_PKG_VERSION"))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("cannot claim preview job: {error}"))?;
    let Some(row) = row else {
        tx.rollback().await.ok();
        lease.release().await;
        return Ok(None);
    };
    let run = ClaimedRun {
        id: row.get(0),
        public_id: row.get(1),
        content_id: row.get(2),
        sha256: row.get(3),
        profile: row.get(4),
        linear_deflection: row.get(5),
        angular_deflection: row.get(6),
    };
    // The session lock survives this commit, so no sweeper can ever observe the
    // claimed row as running without its lease.
    job_lease::hold(&mut tx, job_lease::CONVERSION, run.id).await?;
    tx.commit()
        .await
        .map_err(|error| format!("cannot commit preview claim: {error}"))?;
    Ok(Some((run, lease)))
}
