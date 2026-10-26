//! In-app notification center (ADR 0038).
//!
//! Notifications are derived on request from operations health and recent
//! catalog and audit records; nothing is sent outside VÖLUND. *Conditions*
//! (stale backup, unhealthy worker, low storage) stay until their cause is
//! fixed. *Events* (failed jobs, notable security denials) count as unread
//! until the account opens the center.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Serialize;
use sqlx::{PgPool, Row};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::operations::ComponentStatus;
use crate::session::{AuthenticatedSession, Capability};
use crate::storage_health::OperationalState;

const EVENT_WINDOW_DAYS: i32 = 7;
const MAX_JOB_EVENTS: i64 = 20;
const LOGIN_DENIALS_WARNING: i64 = 5;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub key: String,
    /// `job`, `storage`, `backup`, `worker`, or `security`.
    pub category: &'static str,
    /// `condition` stays until resolved; `event` is read once seen.
    pub kind: &'static str,
    pub severity: &'static str,
    pub title: String,
    pub message: String,
    pub occurred_at_unix_ms: i64,
    /// Browser view that resolves it: `problems`, `administration`, or `model-problems`.
    pub view: &'static str,
    pub model_id: Option<String>,
    pub unread: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    /// Active conditions plus unread events: the navigation badge.
    pub attention: i64,
    pub seen_at_unix_ms: Option<i64>,
}

pub(crate) fn routes() -> axum::Router<ApiState> {
    axum::Router::new()
        .route("/notifications", get(list))
        .route("/notifications/seen", post(mark_seen))
}

fn severity(state: OperationalState) -> Option<&'static str> {
    match state {
        OperationalState::Healthy => None,
        OperationalState::Degraded => Some("warning"),
        OperationalState::Blocked => Some("error"),
    }
}

fn component_notice(
    component: &ComponentStatus,
    category: &'static str,
    title: &str,
    now: i64,
) -> Option<Notification> {
    let severity = severity(component.state)?;
    let last = component.last_succeeded_at_unix_ms.map_or_else(
        || "noch nie erfolgreich".to_owned(),
        |_| "letzter Erfolg liegt zu lange zurück".to_owned(),
    );
    Some(Notification {
        key: format!("{category}:{}", component.key),
        category,
        kind: "condition",
        severity,
        title: title.to_owned(),
        message: format!(
            "{} ({last}; {}).",
            component.key,
            component.reasons.join(", ")
        ),
        occurred_at_unix_ms: component
            .last_failed_at_unix_ms
            .or(component.last_succeeded_at_unix_ms)
            .unwrap_or(now),
        view: "administration",
        model_id: None,
        unread: true,
    })
}

async fn operational(pool: &PgPool) -> Result<Vec<Notification>, String> {
    let health = crate::operations::inspect(pool).await?;
    let now = health.checked_at_unix_ms;
    let mut items: Vec<Notification> = Vec::new();
    items.extend(component_notice(
        &health.backup,
        "backup",
        "Backup veraltet oder fehlgeschlagen",
        now,
    ));
    for worker in &health.workers {
        items.extend(component_notice(
            worker,
            "worker",
            "Hintergrunddienst meldet sich nicht ordnungsgemäß",
            now,
        ));
    }
    for library in &health.libraries {
        if let Some(severity) = severity(library.storage.state) {
            let reasons: Vec<&str> = library
                .storage
                .reasons
                .iter()
                .map(|reason| reason.code)
                .collect();
            items.push(Notification {
                key: format!("storage:{}", library.key),
                category: "storage",
                kind: "condition",
                severity,
                title: format!("Speicher der Bibliothek „{}“", library.name),
                message: format!(
                    "Zustand {}: {}.",
                    if severity == "error" {
                        "blockiert"
                    } else {
                        "eingeschränkt"
                    },
                    reasons.join(", ")
                ),
                occurred_at_unix_ms: library.storage.checked_at_unix_ms,
                view: "administration",
                model_id: None,
                unread: true,
            });
        }
    }
    Ok(items)
}

async fn job_events(pool: &PgPool, include_scans: bool) -> Result<Vec<Notification>, String> {
    let rows = sqlx::query(
        "SELECT run.public_id::text, run.status, round(extract(epoch FROM run.finished_at)*1000)::bigint, \
         source.relative_path, model.public_id::text, model.name \
         FROM volund.conversion_runs run \
         JOIN LATERAL (SELECT source.id, source.relative_path FROM volund.source_files source \
          WHERE source.content_object_id=run.content_object_id ORDER BY source.id LIMIT 1) source ON true \
         LEFT JOIN LATERAL (SELECT model.public_id, model.name FROM volund.model_source_files link \
          JOIN volund.models model ON model.id=link.model_id AND model.active \
          WHERE link.source_file_id=source.id ORDER BY model.id LIMIT 1) model ON true \
         WHERE run.status IN ('failed','timed-out') AND run.finished_at > now() - make_interval(days => $1) \
         ORDER BY run.finished_at DESC, run.id DESC LIMIT $2",
    )
    .bind(EVENT_WINDOW_DAYS)
    .bind(MAX_JOB_EVENTS)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list failed conversions: {error}"))?;
    let mut items: Vec<Notification> = rows
        .iter()
        .map(|row| {
            let status: String = row.get(1);
            let model: Option<String> = row.get(4);
            Notification {
                key: format!("conversion:{}", row.get::<String, _>(0)),
                category: "job",
                kind: "event",
                severity: "warning",
                title: if status == "timed-out" {
                    "Konvertierung abgebrochen (Zeitlimit)"
                } else {
                    "Konvertierung fehlgeschlagen"
                }
                .to_owned(),
                message: format!(
                    "{}{}",
                    row.get::<String, _>(3),
                    row.get::<Option<String>, _>(5)
                        .map_or_else(String::new, |name| format!(" · Modell „{name}“"))
                ),
                occurred_at_unix_ms: row.get(2),
                view: if model.is_some() {
                    "model-problems"
                } else {
                    "problems"
                },
                model_id: model,
                unread: true,
            }
        })
        .collect();
    if include_scans {
        let scans = sqlx::query(
            "SELECT scan.public_id::text, root.display_name, round(extract(epoch FROM scan.finished_at)*1000)::bigint \
             FROM volund.scan_runs scan JOIN volund.library_roots root ON root.id=scan.library_root_id \
             WHERE scan.status='failed' AND scan.finished_at > now() - make_interval(days => $1) \
             ORDER BY scan.finished_at DESC LIMIT $2",
        )
        .bind(EVENT_WINDOW_DAYS)
        .bind(MAX_JOB_EVENTS)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("cannot list failed scans: {error}"))?;
        items.extend(scans.iter().map(|row| Notification {
            key: format!("scan:{}", row.get::<String, _>(0)),
            category: "job",
            kind: "event",
            severity: "error",
            title: "Scan fehlgeschlagen".to_owned(),
            message: format!("Bibliothek „{}“", row.get::<String, _>(1)),
            occurred_at_unix_ms: row.get(2),
            view: "administration",
            model_id: None,
            unread: true,
        }));
    }
    Ok(items)
}

async fn security_events(pool: &PgPool) -> Result<Vec<Notification>, String> {
    let rows = sqlx::query(
        "SELECT action, count(*)::bigint, round(extract(epoch FROM max(occurred_at))*1000)::bigint \
         FROM volund.security_audit_events WHERE outcome='denied' AND occurred_at > now() - interval '24 hours' \
         GROUP BY action ORDER BY action",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot summarize security events: {error}"))?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let action: String = row.get(0);
            let count: i64 = row.get(1);
            let login = action == "session.login";
            if login && count < LOGIN_DENIALS_WARNING {
                return None;
            }
            Some(Notification {
                key: format!("security:{action}"),
                category: "security",
                kind: "event",
                severity: if login { "warning" } else { "info" },
                title: if login {
                    "Gehäufte fehlgeschlagene Anmeldungen".to_owned()
                } else {
                    "Abgewiesene Zugriffe".to_owned()
                },
                message: format!("{count}× „{action}“ in den letzten 24 Stunden."),
                occurred_at_unix_ms: row.get(2),
                view: "administration",
                model_id: None,
                unread: true,
            })
        })
        .collect())
}

/// The notifications `actor` may see, newest and most severe first.
///
/// # Errors
/// Returns a database diagnostic when a source cannot be read.
pub async fn collect(
    pool: &PgPool,
    actor: &AuthenticatedSession,
) -> Result<NotificationPage, String> {
    let admin = actor.has_capability(Capability::LibraryAdmin)
        || actor.has_capability(Capability::SettingsAdmin);
    let mut items = Vec::new();
    if admin {
        items.extend(operational(pool).await?);
        items.extend(security_events(pool).await?);
    }
    if actor.has_capability(Capability::CatalogWrite) {
        items.extend(job_events(pool, admin).await?);
    }
    let seen_at: Option<i64> = sqlx::query_scalar(
        "SELECT round(extract(epoch FROM seen_at)*1000)::bigint FROM volund.notification_reads WHERE user_id=$1",
    )
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load notification marker: {error}"))?;
    for item in &mut items {
        item.unread =
            item.kind == "condition" || seen_at.is_none_or(|seen| item.occurred_at_unix_ms > seen);
    }
    let rank = |severity: &str| match severity {
        "error" => 3,
        "warning" => 2,
        _ => 1,
    };
    items.sort_by(|left, right| {
        right
            .unread
            .cmp(&left.unread)
            .then(rank(right.severity).cmp(&rank(left.severity)))
            .then(right.occurred_at_unix_ms.cmp(&left.occurred_at_unix_ms))
            .then(left.key.cmp(&right.key))
    });
    let attention =
        i64::try_from(items.iter().filter(|item| item.unread).count()).unwrap_or(i64::MAX);
    Ok(NotificationPage {
        items,
        attention,
        seen_at_unix_ms: seen_at,
    })
}

async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Json<NotificationPage>, ApiError> {
    collect(&state.pool, &actor)
        .await
        .map(Json)
        .map_err(ApiError::Database)
}

async fn mark_seen(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<StatusCode, ApiError> {
    sqlx::query(
        "INSERT INTO volund.notification_reads (user_id, seen_at) VALUES ($1, now()) \
         ON CONFLICT (user_id) DO UPDATE SET seen_at = now()",
    )
    .bind(actor.database_user_id())
    .execute(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot mark notifications seen: {error}")))?;
    Ok(StatusCode::NO_CONTENT)
}
