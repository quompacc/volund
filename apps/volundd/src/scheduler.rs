#![allow(clippy::missing_errors_doc)]
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub library_id: String,
    pub name: String,
    pub local_time: String,
    pub time_zone: String,
    pub weekday_mask: i16,
    pub full_scan: bool,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSchedule {
    pub id: String,
    pub library_id: String,
    pub library_name: String,
    pub name: String,
    pub local_time: String,
    pub time_zone: String,
    pub weekday_mask: i16,
    pub full_scan: bool,
    pub enabled: bool,
    pub next_run_at_unix_ms: Option<i64>,
    pub last_scheduled_at_unix_ms: Option<i64>,
    pub last_outcome: Option<String>,
    pub last_scan_id: Option<String>,
    pub last_scan_status: Option<String>,
    pub revision: i64,
}

/// List durable schedules. # Errors Returns a persistence failure.
pub async fn list(pool: &PgPool) -> Result<Vec<ScanSchedule>, String> {
    sqlx::query(SCHEDULE_SELECT)
        .fetch_all(pool)
        .await
        .map(|rows| rows.iter().map(schedule).collect())
        .map_err(|error| format!("cannot list scan schedules: {error}"))
}

/// Create a validated schedule. # Errors Returns validation or persistence failure.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: &ScheduleInput,
    confirmation: &str,
) -> Result<ScanSchedule, String> {
    if confirmation != "APPLY SCHEDULE new" {
        crate::security_audit::denied(
            pool,
            actor,
            "scan-schedule.create",
            "scan-schedule",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact schedule application confirmation is required".to_owned());
    }
    let (time, zone) = validate(input)?;
    let next = input
        .enabled
        .then(|| next_occurrence(Utc::now(), time, zone, input.weekday_mask))
        .transpose()?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let id: String = sqlx::query_scalar("INSERT INTO volund.scan_schedules (library_root_id,name,local_time,time_zone,weekday_mask,full_scan,enabled,next_run_at,updated_by_user_id) SELECT id,$2,$3,$4,$5,$6,$7,$8,$9 FROM volund.library_roots WHERE public_id::text=$1 AND enabled RETURNING public_id::text")
        .bind(&input.library_id).bind(&input.name).bind(time).bind(&input.time_zone).bind(input.weekday_mask)
        .bind(input.full_scan).bind(input.enabled).bind(next).bind(actor.database_user_id())
        .fetch_optional(&mut *tx).await.map_err(|error| format!("cannot create scan schedule: {error}"))?
        .ok_or_else(|| "active library not found".to_owned())?;
    audit(&mut tx, actor, "scan-schedule.create", &id).await?;
    tx.commit().await.map_err(|error| error.to_string())?;
    load(pool, &id).await
}

/// Update with revision control. # Errors Returns validation, conflict, or persistence failure.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    expected_revision: i64,
    input: &ScheduleInput,
    confirmation: &str,
) -> Result<ScanSchedule, String> {
    if confirmation != format!("APPLY SCHEDULE {id}") {
        crate::security_audit::denied(
            pool,
            actor,
            "scan-schedule.update",
            "scan-schedule",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact schedule application confirmation is required".to_owned());
    }
    let (time, zone) = validate(input)?;
    let next = input
        .enabled
        .then(|| next_occurrence(Utc::now(), time, zone, input.weekday_mask))
        .transpose()?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let updated = sqlx::query_scalar::<_,String>("UPDATE volund.scan_schedules schedule SET library_root_id=root.id,name=$3,local_time=$4,time_zone=$5,weekday_mask=$6,full_scan=$7,enabled=$8,next_run_at=$9,revision=schedule.revision+1,updated_by_user_id=$10,updated_at=now() FROM volund.library_roots root WHERE schedule.public_id::text=$1 AND schedule.revision=$2 AND root.public_id::text=$11 AND (root.enabled OR NOT $8) RETURNING schedule.public_id::text")
        .bind(id).bind(expected_revision).bind(&input.name).bind(time).bind(&input.time_zone).bind(input.weekday_mask)
        .bind(input.full_scan).bind(input.enabled).bind(next).bind(actor.database_user_id()).bind(&input.library_id)
        .fetch_optional(&mut *tx).await.map_err(|error| format!("cannot update scan schedule: {error}"))?
        .ok_or_else(|| "schedule, revision, or active library not found".to_owned())?;
    audit(&mut tx, actor, "scan-schedule.update", &updated).await?;
    tx.commit().await.map_err(|error| error.to_string())?;
    load(pool, id).await
}

/// Delete with exact confirmation. # Errors Returns confirmation or persistence failure.
pub async fn delete(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    expected_revision: i64,
    confirmation: &str,
) -> Result<(), String> {
    if confirmation != format!("DELETE SCHEDULE {id}") {
        crate::security_audit::denied(
            pool,
            actor,
            "scan-schedule.delete",
            "scan-schedule",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact schedule deletion confirmation is required".into());
    }
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let deleted = sqlx::query_scalar::<_, String>(
        "DELETE FROM volund.scan_schedules WHERE public_id::text=$1 AND revision=$2 RETURNING public_id::text",
    )
    .bind(id)
    .bind(expected_revision)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("cannot delete scan schedule: {error}"))?
    .ok_or_else(|| "scan schedule not found or revision changed; reload before confirming".to_owned())?;
    audit(&mut tx, actor, "scan-schedule.delete", &deleted).await?;
    tx.commit().await.map_err(|error| error.to_string())
}

/// Coalesce and enqueue due work. # Errors Returns persisted-state failure.
pub async fn process_due(pool: &PgPool) -> Result<u64, String> {
    let mut count = 0;
    loop {
        let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
        let Some(row) = sqlx::query("SELECT schedule.id,schedule.next_run_at,schedule.local_time,schedule.time_zone,schedule.weekday_mask,schedule.full_scan,schedule.library_root_id,root.enabled FROM volund.scan_schedules schedule JOIN volund.library_roots root ON root.id=schedule.library_root_id WHERE schedule.enabled AND schedule.next_run_at<=now() ORDER BY schedule.next_run_at,schedule.id FOR UPDATE OF schedule SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await.map_err(|error| format!("cannot claim due schedule: {error}"))? else { tx.rollback().await.ok(); break; };
        let schedule_id: i64 = row.get(0);
        let due: DateTime<Utc> = row.get(1);
        let local_time: NaiveTime = row.get(2);
        let zone: Tz = row
            .get::<String, _>(3)
            .parse()
            .map_err(|_| "persisted schedule time zone is invalid".to_owned())?;
        let mask: i16 = row.get(4);
        let next = next_occurrence(Utc::now(), local_time, zone, mask)?;
        let (outcome, scan_id): (&str, Option<i64>) = if row.get::<bool, _>(7) {
            let claimed = sqlx::query("INSERT INTO volund.scan_runs (library_root_id,status,full_scan,schedule_id,scheduled_for) VALUES ($1,'queued',$2,$3,$4) ON CONFLICT (library_root_id) WHERE status IN ('queued','running') DO UPDATE SET full_scan=volund.scan_runs.full_scan OR EXCLUDED.full_scan RETURNING id,(xmax=0)")
                .bind(row.get::<i64,_>(6)).bind(row.get::<bool,_>(5)).bind(schedule_id).bind(due)
                .fetch_one(&mut *tx).await.map_err(|error| format!("cannot enqueue scheduled scan: {error}"))?;
            (
                if claimed.get::<bool, _>(1) {
                    "enqueued"
                } else {
                    "coalesced"
                },
                Some(claimed.get(0)),
            )
        } else {
            ("blocked", None)
        };
        sqlx::query("UPDATE volund.scan_schedules SET next_run_at=$2,last_scheduled_at=$3,last_outcome=$4,last_scan_run_id=$5 WHERE id=$1")
            .bind(schedule_id).bind(next).bind(due).bind(outcome).bind(scan_id).execute(&mut *tx).await
            .map_err(|error| format!("cannot advance scan schedule: {error}"))?;
        tx.commit().await.map_err(|error| error.to_string())?;
        count += 1;
    }
    Ok(count)
}

/// Calculate deterministic civil time. # Errors Returns when no future occurrence is representable.
pub fn next_occurrence(
    after: DateTime<Utc>,
    time: NaiveTime,
    zone: Tz,
    mask: i16,
) -> Result<DateTime<Utc>, String> {
    for day in 0..=370 {
        let date = after.with_timezone(&zone).date_naive() + Duration::days(day);
        let weekday = date.weekday().num_days_from_monday();
        if mask & (1_i16 << weekday) == 0 {
            continue;
        }
        let mut local = NaiveDateTime::new(date, time.with_second(0).unwrap_or(time));
        for _ in 0..=180 {
            let candidate = match zone.from_local_datetime(&local) {
                LocalResult::Single(value) => Some(value.with_timezone(&Utc)),
                LocalResult::Ambiguous(first, second) => {
                    Some(first.min(second).with_timezone(&Utc))
                }
                LocalResult::None => None,
            };
            if let Some(candidate) = candidate.filter(|value| *value > after) {
                return Ok(candidate);
            }
            if candidate.is_some() {
                break;
            }
            local += Duration::minutes(1);
        }
    }
    Err("cannot calculate next schedule occurrence".to_owned())
}

fn validate(input: &ScheduleInput) -> Result<(NaiveTime, Tz), String> {
    if input.name.trim() != input.name || input.name.is_empty() || input.name.chars().count() > 80 {
        return Err("schedule name must contain 1 to 80 trimmed characters".into());
    }
    if !(1..=127).contains(&input.weekday_mask) {
        return Err("weekday mask must be between 1 and 127".into());
    }
    let time = NaiveTime::parse_from_str(&input.local_time, "%H:%M")
        .map_err(|_| "local time must use HH:MM".to_owned())?;
    let zone = input
        .time_zone
        .parse()
        .map_err(|_| "time zone must be a supported IANA identifier".to_owned())?;
    Ok((time, zone))
}

async fn load(pool: &PgPool, id: &str) -> Result<ScanSchedule, String> {
    sqlx::query(&format!(
        "{SCHEDULE_SELECT} WHERE schedule.public_id::text=$1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load scan schedule: {error}"))?
    .as_ref()
    .map(schedule)
    .ok_or_else(|| "scan schedule not found".into())
}
fn schedule(row: &sqlx::postgres::PgRow) -> ScanSchedule {
    ScanSchedule {
        id: row.get(0),
        library_id: row.get(1),
        library_name: row.get(2),
        name: row.get(3),
        local_time: row.get::<NaiveTime, _>(4).format("%H:%M").to_string(),
        time_zone: row.get(5),
        weekday_mask: row.get(6),
        full_scan: row.get(7),
        enabled: row.get(8),
        next_run_at_unix_ms: row
            .get::<Option<DateTime<Utc>>, _>(9)
            .map(|v| v.timestamp_millis()),
        last_scheduled_at_unix_ms: row
            .get::<Option<DateTime<Utc>>, _>(10)
            .map(|v| v.timestamp_millis()),
        last_outcome: row.get(11),
        last_scan_id: row.get(12),
        last_scan_status: row.get(13),
        revision: row.get(14),
    }
}
async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    id: &str,
) -> Result<(), String> {
    sqlx::query("INSERT INTO volund.security_audit_events (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) VALUES ($1,$2::uuid,$3,$4,'success','scan-schedule',$5::uuid,$6)").bind(actor.database_user_id()).bind(&actor.user_id).bind(&actor.display_name).bind(action).bind(id).bind(json!({"scheduleId":id})).execute(&mut **tx).await.map(|_|()).map_err(|error|format!("cannot audit scan schedule: {error}"))
}
const SCHEDULE_SELECT: &str = "SELECT schedule.public_id::text,root.public_id::text,root.display_name,schedule.name,schedule.local_time,schedule.time_zone,schedule.weekday_mask,schedule.full_scan,schedule.enabled,schedule.next_run_at,schedule.last_scheduled_at,schedule.last_outcome,run.public_id::text,run.status,schedule.revision FROM volund.scan_schedules schedule JOIN volund.library_roots root ON root.id=schedule.library_root_id LEFT JOIN volund.scan_runs run ON run.id=schedule.last_scan_run_id";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dst_rules_are_stable() {
        let zone: Tz = "Europe/Berlin".parse().unwrap();
        let time = NaiveTime::from_hms_opt(2, 30, 0).unwrap();
        let spring = DateTime::parse_from_rfc3339("2026-03-28T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            next_occurrence(spring, time, zone, 127)
                .unwrap()
                .to_rfc3339(),
            "2026-03-29T01:00:00+00:00"
        );
        let fall = DateTime::parse_from_rfc3339("2026-10-24T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            next_occurrence(fall, time, zone, 127).unwrap().to_rfc3339(),
            "2026-10-25T00:30:00+00:00"
        );
    }
}
