use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::session::AuthenticatedSession;

const MAX_PAGE: i64 = 100;

#[derive(Debug)]
pub enum JobAdminError {
    BadRequest(String),
    Conflict(String),
    NotFound,
    Database(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPage {
    pub items: Vec<Job>,
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub title: String,
    pub context: String,
    pub profile: Option<String>,
    pub attempt: i32,
    pub retry_of_id: Option<String>,
    pub cancellation_requested_at_unix_ms: Option<i64>,
    pub requested_at_unix_ms: i64,
    pub started_at_unix_ms: Option<i64>,
    pub finished_at_unix_ms: Option<i64>,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub diagnostic: Option<&'static str>,
    pub can_retry: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Copy)]
pub struct JobListOptions<'a> {
    pub kind: Option<&'a str>,
    pub status: Option<&'a str>,
    pub limit: i64,
    pub offset: i64,
}

/// List a bounded page from both durable execution tables.
///
/// # Errors
///
/// Returns a validation or database error.
pub async fn list(pool: &PgPool, options: JobListOptions<'_>) -> Result<JobPage, JobAdminError> {
    validate_filters(options)?;
    let rows = sqlx::query(JOB_UNION_QUERY)
        .bind(options.kind)
        .bind(options.status)
        .bind(options.limit)
        .bind(options.offset)
        .bind(Option::<&str>::None)
        .fetch_all(pool)
        .await
        .map_err(database_error("list jobs"))?;
    let total = rows.first().map_or(0, |row| row.get(17));
    Ok(JobPage {
        items: rows.iter().map(job_from_row).collect(),
        limit: options.limit,
        offset: options.offset,
        total,
    })
}

/// Load one job by kind and public ID.
///
/// # Errors
///
/// Returns a validation, not-found, or database error.
pub async fn get(pool: &PgPool, kind: &str, id: &str) -> Result<Job, JobAdminError> {
    validate_kind(kind)?;
    let rows = sqlx::query(JOB_UNION_QUERY)
        .bind(Some(kind))
        .bind(Option::<&str>::None)
        .bind(1_i64)
        .bind(0_i64)
        .bind(Some(id))
        .fetch_all(pool)
        .await
        .map_err(database_error("load job"))?;
    rows.first()
        .map(job_from_row)
        .ok_or(JobAdminError::NotFound)
}

/// Request cancellation with a locked, audited, idempotent transition.
///
/// # Errors
///
/// Returns an error for invalid confirmation/state or unavailable persistence.
pub async fn cancel(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    kind: &str,
    id: &str,
    confirmation: &str,
) -> Result<Job, JobAdminError> {
    if let Err(error) = validate_confirmation("CANCEL", id, confirmation) {
        crate::security_audit::denied(pool, actor, "job.cancel", "job", "confirmation_mismatch")
            .await;
        return Err(error);
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin cancellation"))?;
    let (database_id, previous, current, idempotent) = if kind == "scan" {
        cancel_scan(&mut transaction, id).await?
    } else if kind == "conversion" {
        cancel_conversion(&mut transaction, id).await?
    } else {
        return Err(JobAdminError::BadRequest("unknown job kind".to_owned()));
    };
    audit_action(
        &mut transaction,
        actor,
        "job.cancel",
        kind,
        id,
        json!({"previousStatus": previous, "status": current, "idempotent": idempotent}),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit cancellation"))?;
    let _ = database_id;
    get(pool, kind, id).await
}

/// Create or return one queued retry linked to a terminal attempt.
///
/// # Errors
///
/// Returns an error for invalid confirmation/state or unavailable persistence.
pub async fn retry(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    kind: &str,
    id: &str,
    confirmation: &str,
) -> Result<Job, JobAdminError> {
    if let Err(error) = validate_confirmation("RETRY", id, confirmation) {
        crate::security_audit::denied(pool, actor, "job.retry", "job", "confirmation_mismatch")
            .await;
        return Err(error);
    }
    let mut transaction = pool.begin().await.map_err(database_error("begin retry"))?;
    let (retry_id, previous, idempotent) = if kind == "scan" {
        retry_scan(&mut transaction, id).await?
    } else if kind == "conversion" {
        retry_conversion(&mut transaction, id).await?
    } else {
        return Err(JobAdminError::BadRequest("unknown job kind".to_owned()));
    };
    audit_action(
        &mut transaction,
        actor,
        "job.retry",
        kind,
        id,
        json!({"previousStatus": previous, "retryId": retry_id, "idempotent": idempotent}),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit retry"))?;
    get(pool, kind, &retry_id).await
}

async fn cancel_scan(
    transaction: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(i64, String, String, bool), JobAdminError> {
    let row = lock_row(transaction, "scan", id).await?;
    let database_id: i64 = row.get(0);
    let status: String = row.get(1);
    if status == "cancelled" {
        return Ok((database_id, status.clone(), status, true));
    }
    if !matches!(status.as_str(), "queued" | "running") {
        return Err(JobAdminError::Conflict(
            "only queued or running jobs can be cancelled".to_owned(),
        ));
    }
    let next = if status == "queued" {
        "cancelled"
    } else {
        "running"
    };
    sqlx::query(
        "UPDATE volund.scan_runs SET cancellation_requested_at = coalesce(cancellation_requested_at, now()), \
         status = CASE WHEN status = 'queued' THEN 'cancelled' ELSE status END, \
         finished_at = CASE WHEN status = 'queued' THEN now() ELSE finished_at END WHERE id = $1",
    )
    .bind(database_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("cancel scan"))?;
    Ok((database_id, status, next.to_owned(), false))
}

async fn cancel_conversion(
    transaction: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(i64, String, String, bool), JobAdminError> {
    let row = lock_row(transaction, "conversion", id).await?;
    let database_id: i64 = row.get(0);
    let status: String = row.get(1);
    if status == "cancelled" {
        return Ok((database_id, status.clone(), status, true));
    }
    if !matches!(status.as_str(), "queued" | "running") {
        return Err(JobAdminError::Conflict(
            "only queued or running jobs can be cancelled".to_owned(),
        ));
    }
    let next = if status == "queued" {
        "cancelled"
    } else {
        "running"
    };
    sqlx::query(
        "UPDATE volund.conversion_runs SET cancellation_requested_at = coalesce(cancellation_requested_at, now()), \
         status = CASE WHEN status = 'queued' THEN 'cancelled' ELSE status END, \
         finished_at = CASE WHEN status = 'queued' THEN now() ELSE finished_at END WHERE id = $1",
    )
    .bind(database_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("cancel conversion"))?;
    Ok((database_id, status, next.to_owned(), false))
}

async fn retry_scan(
    transaction: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(String, String, bool), JobAdminError> {
    let row = lock_row(transaction, "scan", id).await?;
    let database_id: i64 = row.get(0);
    let status: String = row.get(1);
    if !matches!(status.as_str(), "failed" | "cancelled") {
        return Err(JobAdminError::Conflict(
            "only failed or cancelled scans can be retried".to_owned(),
        ));
    }
    if let Some(existing) = sqlx::query_scalar::<_, String>(
        "SELECT public_id::text FROM volund.scan_runs WHERE retry_of_id = $1",
    )
    .bind(database_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("load scan retry"))?
    {
        return Ok((existing, status, true));
    }
    let retry_id = sqlx::query_scalar(
        "INSERT INTO volund.scan_runs (library_root_id, status, full_scan, attempt, retry_of_id) \
         SELECT library_root_id, 'queued', full_scan, attempt + 1, id FROM volund.scan_runs WHERE id = $1 \
         RETURNING public_id::text",
    )
    .bind(database_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(conflict_or_database("create scan retry"))?;
    Ok((retry_id, status, false))
}

async fn retry_conversion(
    transaction: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(String, String, bool), JobAdminError> {
    let row = lock_row(transaction, "conversion", id).await?;
    let database_id: i64 = row.get(0);
    let status: String = row.get(1);
    if !matches!(status.as_str(), "failed" | "cancelled" | "timed-out") {
        return Err(JobAdminError::Conflict(
            "only failed, cancelled, or timed-out conversions can be retried".to_owned(),
        ));
    }
    if let Some(existing) = sqlx::query_scalar::<_, String>(
        "SELECT public_id::text FROM volund.conversion_runs WHERE retry_of_id = $1",
    )
    .bind(database_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("load conversion retry"))?
    {
        return Ok((existing, status, true));
    }
    let retry_id = sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs (content_object_id, converter_name, converter_version, \
         contract_version, profile, status, settings, attempt, retry_of_id, conversion_profile_id, conversion_profile_revision, profile_snapshot) SELECT content_object_id, \
         converter_name, $2, contract_version, profile, 'queued', settings, attempt + 1, id, conversion_profile_id, conversion_profile_revision, profile_snapshot \
         FROM volund.conversion_runs WHERE id = $1 RETURNING public_id::text",
    )
    .bind(database_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .fetch_one(&mut **transaction)
    .await
    .map_err(conflict_or_database("create conversion retry"))?;
    Ok((retry_id, status, false))
}

async fn lock_row(
    transaction: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: &str,
) -> Result<sqlx::postgres::PgRow, JobAdminError> {
    let query = if kind == "scan" {
        "SELECT id, status FROM volund.scan_runs WHERE public_id::text = $1 FOR UPDATE"
    } else {
        "SELECT id, status FROM volund.conversion_runs WHERE public_id::text = $1 FOR UPDATE"
    };
    sqlx::query(query)
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("lock job"))?
        .ok_or(JobAdminError::NotFound)
}

async fn audit_action(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    kind: &str,
    id: &str,
    metadata: serde_json::Value,
) -> Result<(), JobAdminError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events (actor_user_id, actor_public_id, \
         actor_display_name, action, outcome, target_type, target_public_id, metadata) \
         VALUES ($1, $2::uuid, $3, $4, 'success', $5, $6::uuid, $7)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(format!("{kind}-job"))
    .bind(id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("audit job action"))?;
    Ok(())
}

fn job_from_row(row: &sqlx::postgres::PgRow) -> Job {
    let kind: String = row.get(1);
    let status: String = row.get(2);
    let has_diagnostic: bool = row.get(16);
    Job {
        id: row.get(0),
        kind: kind.clone(),
        status: status.clone(),
        title: row.get(3),
        context: row.get(4),
        profile: row.get(5),
        attempt: row.get(6),
        retry_of_id: row.get(7),
        cancellation_requested_at_unix_ms: row.get(8),
        requested_at_unix_ms: row.get(9),
        started_at_unix_ms: row.get(10),
        finished_at_unix_ms: row.get(11),
        progress_current: row.get(12),
        progress_total: row.get(13),
        diagnostic: has_diagnostic.then_some(if kind == "scan" {
            "Scan fehlgeschlagen; Diagnose ist im Support-Protokoll verfügbar."
        } else {
            "Konvertierung fehlgeschlagen; Diagnose ist im Support-Protokoll verfügbar."
        }),
        can_retry: retryable(&kind, &status),
        can_cancel: matches!(status.as_str(), "queued" | "running"),
    }
}

fn validate_filters(options: JobListOptions<'_>) -> Result<(), JobAdminError> {
    if let Some(kind) = options.kind {
        validate_kind(kind)?;
    }
    if options
        .status
        .is_some_and(|value| value.is_empty() || value.len() > 24)
    {
        return Err(JobAdminError::BadRequest(
            "invalid job status filter".to_owned(),
        ));
    }
    if !(1..=MAX_PAGE).contains(&options.limit) || options.offset < 0 {
        return Err(JobAdminError::BadRequest(
            "invalid job page bounds".to_owned(),
        ));
    }
    Ok(())
}

fn validate_kind(kind: &str) -> Result<(), JobAdminError> {
    if matches!(kind, "scan" | "conversion") {
        Ok(())
    } else {
        Err(JobAdminError::BadRequest("unknown job kind".to_owned()))
    }
}

fn validate_confirmation(action: &str, id: &str, confirmation: &str) -> Result<(), JobAdminError> {
    if confirmation == format!("{action} {id}") {
        Ok(())
    } else {
        Err(JobAdminError::BadRequest(
            "job confirmation text does not match".to_owned(),
        ))
    }
}

fn retryable(kind: &str, status: &str) -> bool {
    matches!(
        (kind, status),
        ("scan", "failed" | "cancelled") | ("conversion", "failed" | "cancelled" | "timed-out")
    )
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> JobAdminError {
    move |error| JobAdminError::Database(format!("{context}: {error}"))
}

fn conflict_or_database(context: &'static str) -> impl FnOnce(sqlx::Error) -> JobAdminError {
    move |error| {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            JobAdminError::Conflict("another active or retry job already exists".to_owned())
        } else {
            JobAdminError::Database(format!("{context}: {error}"))
        }
    }
}

const JOB_UNION_QUERY: &str = "WITH jobs AS ( \
 SELECT scan.public_id::text id, 'scan' kind, scan.status, root.display_name title, \
 root.root_key context, NULL::text profile, scan.attempt, parent.public_id::text retry_of_id, \
 (extract(epoch FROM scan.cancellation_requested_at) * 1000)::bigint cancelled_ms, \
 (extract(epoch FROM scan.requested_at) * 1000)::bigint requested_ms, \
 (extract(epoch FROM scan.started_at) * 1000)::bigint started_ms, \
 (extract(epoch FROM scan.finished_at) * 1000)::bigint finished_ms, scan.hashed_files progress_current, \
 CASE WHEN scan.discovered_files > 0 THEN scan.discovered_files END progress_total, \
 scan.error_message IS NOT NULL has_diagnostic, scan.requested_at ordering \
 FROM volund.scan_runs scan JOIN volund.library_roots root ON root.id = scan.library_root_id \
 LEFT JOIN volund.scan_runs parent ON parent.id = scan.retry_of_id UNION ALL \
 SELECT run.public_id::text, 'conversion', run.status, \
 coalesce((SELECT source.relative_path FROM volund.source_files source WHERE source.content_object_id = run.content_object_id ORDER BY source.missing_at NULLS FIRST, source.id LIMIT 1), 'CAD preview'), \
 coalesce((SELECT root.root_key FROM volund.source_files source JOIN volund.library_roots root ON root.id = source.library_root_id WHERE source.content_object_id = run.content_object_id ORDER BY source.missing_at NULLS FIRST, source.id LIMIT 1), 'unavailable'), \
 run.profile, run.attempt, parent.public_id::text, \
 (extract(epoch FROM run.cancellation_requested_at) * 1000)::bigint, \
 (extract(epoch FROM run.requested_at) * 1000)::bigint, \
 (extract(epoch FROM run.started_at) * 1000)::bigint, \
 (extract(epoch FROM run.finished_at) * 1000)::bigint, \
 CASE WHEN run.status IN ('ready','partial') THEN 100 ELSE 0 END, \
 CASE WHEN run.status IN ('ready','partial') THEN 100::bigint END, \
 run.diagnostics <> '[]'::jsonb, run.requested_at FROM volund.conversion_runs run \
 LEFT JOIN volund.conversion_runs parent ON parent.id = run.retry_of_id) \
 SELECT id, kind, status, title, context, profile, attempt, retry_of_id, cancelled_ms, requested_ms, \
 started_ms, finished_ms, progress_current, progress_total, ordering, id, has_diagnostic, \
 count(*) OVER()::bigint total FROM jobs WHERE ($1::text IS NULL OR kind = $1) \
 AND ($2::text IS NULL OR status = $2) AND ($5::text IS NULL OR id = $5) \
 ORDER BY ordering DESC, id DESC LIMIT $3 OFFSET $4";

#[cfg(test)]
mod tests {
    use super::{
        JobAdminError, JobListOptions, retryable, validate_confirmation, validate_filters,
    };

    #[test]
    fn filters_confirmations_and_capabilities_are_bounded() {
        assert!(
            validate_filters(JobListOptions {
                kind: Some("scan"),
                status: None,
                limit: 100,
                offset: 0
            })
            .is_ok()
        );
        assert!(
            validate_filters(JobListOptions {
                kind: Some("other"),
                status: None,
                limit: 1,
                offset: 0
            })
            .is_err()
        );
        assert!(matches!(
            validate_confirmation("CANCEL", "id", "cancel id"),
            Err(JobAdminError::BadRequest(_))
        ));
        assert!(validate_confirmation("RETRY", "id", "RETRY id").is_ok());
        assert!(retryable("conversion", "timed-out"));
        assert!(!retryable("scan", "completed"));
    }
}
