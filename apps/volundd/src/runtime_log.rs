use sqlx::PgPool;

pub use crate::operational_log::Severity;
use crate::operational_log::{self, Event};

/// Record one runtime-owner event with safe job/run correlation.
pub async fn record(
    pool: &PgPool,
    component: &str,
    event: &str,
    code: &str,
    severity: Severity,
    message: &str,
    run_or_job_id: Option<&str>,
) {
    let correlation_is_run = matches!(component, "scheduler" | "retention" | "import-cleanup");
    let _ = operational_log::record(
        pool,
        &Event {
            severity,
            event,
            component,
            code,
            message,
            request_id: None,
            job_id: if correlation_is_run {
                None
            } else {
                run_or_job_id
            },
            run_id: if correlation_is_run {
                run_or_job_id
            } else {
                None
            },
            actor_id: None,
        },
    )
    .await;
}

/// Emit a startup/configuration failure before a database connection exists.
pub fn failure(component: &'static str, event: &'static str, code: &'static str, message: &str) {
    operational_log::emit(&Event {
        severity: Severity::Error,
        event,
        component,
        code,
        message,
        request_id: None,
        job_id: None,
        run_id: None,
        actor_id: None,
    });
}
