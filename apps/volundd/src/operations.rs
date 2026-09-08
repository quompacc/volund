use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sqlx::{PgPool, Row};

use crate::database;
use crate::library_admin::{self, ManagedLibrary};
use crate::storage_health::OperationalState;

const WORKER_DEGRADED_AFTER_SECONDS: i64 = 60;
const WORKER_BLOCKED_AFTER_SECONDS: i64 = 300;
const BACKUP_DEGRADED_AFTER_SECONDS: i64 = 36 * 60 * 60;
const BACKUP_BLOCKED_AFTER_SECONDS: i64 = 48 * 60 * 60;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationsHealth {
    pub state: OperationalState,
    pub checked_at_unix_ms: i64,
    pub version: &'static str,
    pub database: DatabaseStatus,
    pub workers: Vec<ComponentStatus>,
    pub backup: ComponentStatus,
    pub libraries: Vec<ManagedLibrary>,
    pub recent_scans: Vec<RecentScan>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatus {
    pub state: OperationalState,
    pub server_version: i32,
    pub schema_tables: i64,
    pub expected_schema_tables: i64,
    pub applied_migrations: i64,
    pub expected_migrations: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatus {
    pub key: &'static str,
    pub state: OperationalState,
    pub last_outcome: Option<String>,
    pub last_succeeded_at_unix_ms: Option<i64>,
    pub last_failed_at_unix_ms: Option<i64>,
    pub expected_interval_seconds: i64,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentScan {
    pub id: String,
    pub library_key: String,
    pub status: String,
    pub full: bool,
    pub requested_at_unix_ms: i64,
    pub finished_at_unix_ms: Option<i64>,
    pub has_error: bool,
}

/// Record a successful or failed component invocation without storing output.
///
/// # Errors
///
/// Returns a database error when the durable heartbeat cannot be updated.
pub async fn record_component(
    pool: &PgPool,
    component: &str,
    succeeded: bool,
) -> Result<(), String> {
    let outcome = if succeeded { "success" } else { "failed" };
    sqlx::query(
        "INSERT INTO volund.operational_components \
         (component_key, last_outcome, last_succeeded_at, last_failed_at) \
         VALUES ($1, $2, CASE WHEN $3 THEN now() END, CASE WHEN $3 THEN NULL ELSE now() END) \
         ON CONFLICT (component_key) DO UPDATE SET \
          last_outcome = EXCLUDED.last_outcome, \
          last_succeeded_at = CASE WHEN $3 THEN now() ELSE volund.operational_components.last_succeeded_at END, \
          last_failed_at = CASE WHEN $3 THEN volund.operational_components.last_failed_at ELSE now() END, \
          updated_at = now() \
         WHERE NOT $3 OR volund.operational_components.last_outcome <> 'success' \
          OR volund.operational_components.updated_at < now() - interval '30 seconds'",
    )
    .bind(component)
    .bind(outcome)
    .bind(succeeded)
    .execute(pool)
    .await
    .map_err(|error| format!("cannot record {component} heartbeat: {error}"))?;
    Ok(())
}

/// Build the authorized aggregate operations view.
///
/// # Errors
///
/// Returns an error when database, library, heartbeat, or scan state cannot be
/// read.
pub async fn inspect(pool: &PgPool) -> Result<OperationsHealth, String> {
    let checked_at_unix_ms = now_unix_ms();
    let db = database::inspect(pool).await?;
    let database_state = if db.is_ready() {
        OperationalState::Healthy
    } else {
        OperationalState::Blocked
    };
    let database = DatabaseStatus {
        state: database_state,
        server_version: db.server_version_num,
        schema_tables: db.schema_table_count,
        expected_schema_tables: database::EXPECTED_SCHEMA_TABLES,
        applied_migrations: db.applied_migrations,
        expected_migrations: database::EXPECTED_MIGRATIONS,
    };
    let libraries = library_admin::list(pool)
        .await
        .map_err(|error| format!("cannot inspect library operations: {error:?}"))?;
    let workers = vec![
        component_status(pool, "preview-worker", 10, checked_at_unix_ms).await?,
        component_status(pool, "scan-worker", 5, checked_at_unix_ms).await?,
        component_status(pool, "scheduler", 30, checked_at_unix_ms).await?,
        load_component(
            pool,
            "retention-worker",
            24 * 60 * 60,
            checked_at_unix_ms,
            true,
        )
        .await?,
        component_status(pool, "import-cleanup", 60 * 60, checked_at_unix_ms).await?,
    ];
    let backup = backup_status(pool, checked_at_unix_ms).await?;
    let recent_scans = recent_scans(pool).await?;
    let state = std::iter::once(database.state)
        .chain(workers.iter().map(|component| component.state))
        .chain(std::iter::once(backup.state))
        .chain(libraries.iter().map(|library| library.storage.state))
        .max()
        .unwrap_or(OperationalState::Healthy);
    Ok(OperationsHealth {
        state,
        checked_at_unix_ms,
        version: env!("CARGO_PKG_VERSION"),
        database,
        workers,
        backup,
        libraries,
        recent_scans,
    })
}

async fn component_status(
    pool: &PgPool,
    key: &'static str,
    expected_interval_seconds: i64,
    now_ms: i64,
) -> Result<ComponentStatus, String> {
    load_component(pool, key, expected_interval_seconds, now_ms, false).await
}

async fn backup_status(pool: &PgPool, now_ms: i64) -> Result<ComponentStatus, String> {
    load_component(pool, "backup", 24 * 60 * 60, now_ms, true).await
}

async fn load_component(
    pool: &PgPool,
    key: &'static str,
    expected_interval_seconds: i64,
    now_ms: i64,
    backup: bool,
) -> Result<ComponentStatus, String> {
    let row = sqlx::query(
        "SELECT last_outcome, \
         (extract(epoch FROM last_succeeded_at) * 1000)::bigint, \
         (extract(epoch FROM last_failed_at) * 1000)::bigint \
         FROM volund.operational_components WHERE component_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot inspect {key} heartbeat: {error}"))?;
    let last_outcome: Option<String> = row.as_ref().map(|value| value.get(0));
    let last_succeeded_at_unix_ms: Option<i64> = row.as_ref().and_then(|value| value.get(1));
    let last_failed_at_unix_ms: Option<i64> = row.as_ref().and_then(|value| value.get(2));
    let age_seconds = last_succeeded_at_unix_ms.map(|timestamp| (now_ms - timestamp).max(0) / 1000);
    let (degraded_after, blocked_after) = component_thresholds(expected_interval_seconds, backup);
    let (state, reasons) = classify_component(
        last_outcome.as_deref(),
        age_seconds,
        degraded_after,
        blocked_after,
    );
    Ok(ComponentStatus {
        key,
        state,
        last_outcome,
        last_succeeded_at_unix_ms,
        last_failed_at_unix_ms,
        expected_interval_seconds,
        reasons,
    })
}

fn component_thresholds(expected_interval_seconds: i64, backup: bool) -> (i64, i64) {
    if backup {
        return (BACKUP_DEGRADED_AFTER_SECONDS, BACKUP_BLOCKED_AFTER_SECONDS);
    }
    (
        WORKER_DEGRADED_AFTER_SECONDS.max(expected_interval_seconds.saturating_mul(2)),
        WORKER_BLOCKED_AFTER_SECONDS.max(expected_interval_seconds.saturating_mul(5)),
    )
}

fn classify_component(
    outcome: Option<&str>,
    success_age_seconds: Option<i64>,
    degraded_after: i64,
    blocked_after: i64,
) -> (OperationalState, Vec<&'static str>) {
    if outcome.is_none() || success_age_seconds.is_none() {
        return (OperationalState::Blocked, vec!["component_never_succeeded"]);
    }
    if outcome == Some("failed") {
        return (OperationalState::Blocked, vec!["component_last_run_failed"]);
    }
    let age = success_age_seconds.unwrap_or_default();
    if age > blocked_after {
        (
            OperationalState::Blocked,
            vec!["component_heartbeat_expired"],
        )
    } else if age > degraded_after {
        (
            OperationalState::Degraded,
            vec!["component_heartbeat_stale"],
        )
    } else {
        (OperationalState::Healthy, Vec::new())
    }
}

async fn recent_scans(pool: &PgPool) -> Result<Vec<RecentScan>, String> {
    let rows = sqlx::query(
        "SELECT scan.public_id::text, root.root_key, scan.status, scan.full_scan, \
         (extract(epoch FROM scan.requested_at) * 1000)::bigint, \
         (extract(epoch FROM scan.finished_at) * 1000)::bigint, \
         scan.error_message IS NOT NULL \
         FROM volund.scan_runs scan JOIN volund.library_roots root ON root.id = scan.library_root_id \
         ORDER BY scan.requested_at DESC, scan.id DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot inspect recent scans: {error}"))?;
    Ok(rows
        .iter()
        .map(|row| RecentScan {
            id: row.get(0),
            library_key: row.get(1),
            status: row.get(2),
            full: row.get(3),
            requested_at_unix_ms: row.get(4),
            finished_at_unix_ms: row.get(5),
            has_error: row.get(6),
        })
        .collect())
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::{classify_component, component_thresholds};
    use crate::storage_health::OperationalState;

    #[test]
    fn component_age_and_failure_map_to_shared_states() {
        assert_eq!(
            classify_component(Some("success"), Some(10), 60, 300).0,
            OperationalState::Healthy
        );
        assert_eq!(
            classify_component(Some("success"), Some(61), 60, 300).0,
            OperationalState::Degraded
        );
        assert_eq!(
            classify_component(Some("success"), Some(301), 60, 300).0,
            OperationalState::Blocked
        );
        assert_eq!(
            classify_component(Some("failed"), Some(1), 60, 300).0,
            OperationalState::Blocked
        );
        assert_eq!(
            classify_component(None, None, 60, 300).0,
            OperationalState::Blocked
        );
    }

    #[test]
    fn slow_worker_thresholds_follow_the_declared_interval() {
        assert_eq!(component_thresholds(10, false), (60, 300));
        assert_eq!(
            component_thresholds(60 * 60, false),
            (2 * 60 * 60, 5 * 60 * 60)
        );
        assert_eq!(
            component_thresholds(24 * 60 * 60, true),
            (36 * 60 * 60, 48 * 60 * 60)
        );
    }
}
