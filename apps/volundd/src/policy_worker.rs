#![allow(clippy::missing_errors_doc)]
use std::path::PathBuf;

use sqlx::PgPool;

use crate::{operations, retention, scheduler};

/// Run and heartbeat the scheduler: due scans, source cleanup and the
/// automatic thumbnail backfill (ADR 0040). Returns started scans plus newly
/// queued thumbnail conversions. # Errors Returns scheduling or heartbeat failure.
pub async fn run_scheduler(pool: &PgPool) -> Result<u64, String> {
    let cleanup = crate::source_cleanup::reconcile(pool).await;
    let thumbnails = crate::thumbnail_backfill::enqueue_missing(
        pool,
        crate::thumbnail_backfill::MAX_ACTIVE_CONVERSIONS,
    )
    .await;
    let result = scheduler::process_due(pool)
        .await
        .and_then(|count| cleanup.map(|_| count))
        .and_then(|count| thumbnails.map(|queued| count + queued));
    operations::record_component(pool, "scheduler", result.is_ok()).await?;
    result
}

/// Run and heartbeat retention. # Errors Returns cleanup or heartbeat failure.
pub async fn run_retention(
    pool: &PgPool,
    derived_root: PathBuf,
) -> Result<retention::RetentionResult, String> {
    let result = retention::execute(pool, &derived_root, None, None).await;
    operations::record_component(pool, "retention-worker", result.is_ok()).await?;
    result
}
