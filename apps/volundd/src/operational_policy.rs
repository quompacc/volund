use serde_json::{Value, json};
use sqlx::PgPool;

use crate::settings::{self, Definition, SettingsError};

#[derive(Clone, Copy)]
pub struct OperationalPolicy {
    pub scan_concurrency: i64,
    pub conversion_concurrency: i64,
    pub derived_artifact_days: i64,
    pub job_diagnostic_days: i64,
    pub max_artifact_runs: i64,
}

/// Resolve settings used by native workers.
///
/// # Errors
/// Returns an invalid or unavailable settings error.
pub async fn resolve(pool: &PgPool) -> Result<OperationalPolicy, SettingsError> {
    let values = settings::list(pool).await?;
    let integer = |key: &str| {
        values
            .iter()
            .find(|item| item.key == key)
            .and_then(|item| item.value.as_i64())
            .ok_or_else(|| SettingsError::BadRequest(format!("missing integer setting: {key}")))
    };
    Ok(OperationalPolicy {
        scan_concurrency: integer("jobs.scanConcurrency")?,
        conversion_concurrency: integer("jobs.conversionConcurrency")?,
        derived_artifact_days: integer("retention.derivedArtifactDays")?,
        job_diagnostic_days: integer("retention.jobDiagnosticDays")?,
        max_artifact_runs: integer("retention.maxArtifactRuns")?,
    })
}

pub(crate) fn definitions() -> Vec<Definition> {
    vec![
        integer(
            "jobs.scanConcurrency",
            "jobs",
            1,
            1,
            8,
            "workers",
            concurrency,
        ),
        integer(
            "jobs.conversionConcurrency",
            "jobs",
            1,
            1,
            8,
            "workers",
            concurrency,
        ),
        integer(
            "retention.derivedArtifactDays",
            "retention",
            90,
            1,
            3650,
            "days",
            retention_days,
        ),
        integer(
            "retention.jobDiagnosticDays",
            "retention",
            180,
            1,
            3650,
            "days",
            retention_days,
        ),
        integer(
            "retention.maxArtifactRuns",
            "retention",
            10_000,
            10,
            100_000,
            "runs",
            retention_runs,
        ),
    ]
}
fn integer(
    key: &'static str,
    domain: &'static str,
    default: i64,
    minimum: i64,
    maximum: i64,
    unit: &'static str,
    validate: fn(&Value) -> Result<(), SettingsError>,
) -> Definition {
    Definition {
        key,
        domain,
        value_type: "integer",
        default: json!(default),
        constraints: json!({"minimum":minimum,"maximum":maximum,"unit":unit}),
        validate,
        environment: None,
        editable: true,
        sensitive: false,
        effect: "immediate",
    }
}
fn concurrency(value: &Value) -> Result<(), SettingsError> {
    range(value, 1, 8, "worker concurrency")
}
fn retention_days(value: &Value) -> Result<(), SettingsError> {
    range(value, 1, 3650, "retention age")
}
fn retention_runs(value: &Value) -> Result<(), SettingsError> {
    range(value, 10, 100_000, "retained artifact runs")
}
fn range(value: &Value, min: i64, max: i64, name: &str) -> Result<(), SettingsError> {
    if value
        .as_i64()
        .is_some_and(|number| (min..=max).contains(&number))
    {
        Ok(())
    } else {
        Err(SettingsError::BadRequest(format!(
            "{name} must be an integer between {min} and {max}"
        )))
    }
}
