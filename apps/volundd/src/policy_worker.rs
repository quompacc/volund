#![allow(clippy::missing_errors_doc)]
use std::path::PathBuf;

use sqlx::PgPool;

use crate::{operations, retention, scheduler};

/// Run and heartbeat the scheduler. # Errors Returns scheduling or heartbeat failure.
pub async fn run_scheduler(pool: &PgPool) -> Result<u64, String> {
    let cleanup = crate::source_cleanup::reconcile(pool).await;
    let result = scheduler::process_due(pool)
        .await
        .and_then(|count| cleanup.map(|_| count));
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
