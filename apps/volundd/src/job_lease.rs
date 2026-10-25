//! Worker leases for claimed queue jobs.
//!
//! A worker claims a job on a dedicated, detached database connection and takes
//! a session-level advisory lock keyed by the job id inside the claim
//! transaction. The lock outlives that transaction and disappears with the
//! connection, so a worker that is killed, runs out of memory, or loses its host
//! releases every lease immediately. Any later worker treats a `running` job
//! whose lease it can acquire as orphaned. See ADR 0030.

use sqlx::{Connection, PgConnection, PgPool};

/// Advisory-lock class for conversion-run leases.
pub(crate) const CONVERSION: i32 = 860_756_369;
/// Advisory-lock class for scan-run leases.
pub(crate) const SCAN: i32 = 860_756_370;

/// SQL expression mapping a bigint job id (`$2`) onto the int4 lock object id.
const OBJECT_ID: &str = "($2 % 2147483648)::integer";

/// A dedicated connection that holds the advisory locks of one worker task.
pub(crate) struct Lease {
    connection: PgConnection,
}

impl Lease {
    /// Open a connection that is detached from the pool, so dropping it closes
    /// the session and releases its locks instead of returning them to the pool.
    pub(crate) async fn open(pool: &PgPool) -> Result<Self, String> {
        let connection = pool
            .acquire()
            .await
            .map_err(|error| format!("cannot open worker lease connection: {error}"))?
            .detach();
        Ok(Self { connection })
    }

    pub(crate) fn connection(&mut self) -> &mut PgConnection {
        &mut self.connection
    }

    /// Release every lock by closing the session.
    pub(crate) async fn release(self) {
        let _ = self.connection.close().await;
    }
}

/// Take the lease of `job_id` in `class`; call inside the claim transaction.
pub(crate) async fn hold(
    connection: &mut PgConnection,
    class: i32,
    job_id: i64,
) -> Result<(), String> {
    sqlx::query(&format!("SELECT pg_advisory_lock($1, {OBJECT_ID})"))
        .bind(class)
        .bind(job_id)
        .execute(connection)
        .await
        .map_err(|error| format!("cannot take worker lease: {error}"))?;
    Ok(())
}

/// Try to take the lease of a job; `true` means no live worker holds it.
pub(crate) async fn try_hold(
    connection: &mut PgConnection,
    class: i32,
    job_id: i64,
) -> Result<bool, String> {
    sqlx::query_scalar(&format!("SELECT pg_try_advisory_lock($1, {OBJECT_ID})"))
        .bind(class)
        .bind(job_id)
        .fetch_one(connection)
        .await
        .map_err(|error| format!("cannot inspect worker lease: {error}"))
}

/// Release a lease taken with [`try_hold`] on a connection that stays open.
pub(crate) async fn drop_hold(
    connection: &mut PgConnection,
    class: i32,
    job_id: i64,
) -> Result<(), String> {
    sqlx::query(&format!("SELECT pg_advisory_unlock($1, {OBJECT_ID})"))
        .bind(class)
        .bind(job_id)
        .execute(connection)
        .await
        .map_err(|error| format!("cannot release worker lease: {error}"))?;
    Ok(())
}
