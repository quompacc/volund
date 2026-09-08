use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};

use crate::api::ApiState;
use crate::session::AuthenticatedSession;

const MAX_MESSAGE_BYTES: usize = 512;
const MAX_EVENTS: i64 = 500;
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Debug,
    Info,
    Warning,
    Error,
}

impl Severity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Event<'a> {
    pub severity: Severity,
    pub event: &'a str,
    pub component: &'a str,
    pub code: &'a str,
    pub message: &'a str,
    pub request_id: Option<&'a str>,
    pub job_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub actor_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalEvent<'a> {
    timestamp: String,
    severity: Severity,
    event: &'a str,
    component: &'a str,
    code: &'a str,
    version: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEvent {
    pub timestamp: String,
    pub severity: String,
    pub event: String,
    pub component: String,
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
    pub job_id: Option<String>,
    pub run_id: Option<String>,
}

/// Emit one sanitized JSON event to the native service journal.
pub fn emit(event: &Event<'_>) {
    let journal = JournalEvent {
        timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        severity: event.severity,
        event: event.event,
        component: event.component,
        code: event.code,
        version: env!("CARGO_PKG_VERSION"),
        message: sanitize(event.message),
        request_id: bounded_id(event.request_id, 64),
        job_id: bounded_id(event.job_id, 128),
        run_id: bounded_id(event.run_id, 128),
    };
    let line = serde_json::to_string(&journal).unwrap_or_else(|_| {
        "{\"severity\":\"error\",\"event\":\"logging.serialization_failed\",\"code\":\"log_encode_failed\"}".to_owned()
    });
    if event.severity == Severity::Error {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

/// Emit and durably record one bounded event.
///
/// # Errors
///
/// Returns a safe error if the event cannot be inserted.
pub async fn record(pool: &PgPool, event: &Event<'_>) -> Result<(), String> {
    emit(event);
    let message = sanitize(event.message);
    sqlx::query(
        "INSERT INTO volund.operational_log_events \
         (severity,event_name,component,code,message,request_id,job_id,run_id,actor_public_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::uuid)",
    )
    .bind(event.severity.as_str())
    .bind(event.event)
    .bind(event.component)
    .bind(event.code)
    .bind(message)
    .bind(bounded_id(event.request_id, 64))
    .bind(bounded_id(event.job_id, 128))
    .bind(bounded_id(event.run_id, 128))
    .bind(event.actor_id)
    .execute(pool)
    .await
    .map_err(|_| "cannot record sanitized operational event".to_owned())?;
    Ok(())
}

/// Return the recent allowlisted events used by support bundles.
///
/// # Errors
///
/// Returns a safe error when the bounded query fails.
pub async fn recent(pool: &PgPool) -> Result<Vec<StoredEvent>, String> {
    let rows = sqlx::query(
        "SELECT to_char(occurred_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), \
         severity,event_name,component,code,message,request_id,job_id,run_id \
         FROM volund.operational_log_events \
         WHERE occurred_at >= now() - interval '24 hours' \
         ORDER BY occurred_at DESC,id DESC LIMIT $1",
    )
    .bind(MAX_EVENTS)
    .fetch_all(pool)
    .await
    .map_err(|_| "cannot read sanitized operational events".to_owned())?;
    Ok(rows
        .iter()
        .map(|row| StoredEvent {
            timestamp: row.get(0),
            severity: row.get(1),
            event: row.get(2),
            component: row.get(3),
            code: row.get(4),
            message: row.get(5),
            request_id: row.get(6),
            job_id: row.get(7),
            run_id: row.get(8),
        })
        .collect())
}

/// Attach a bounded request ID and expose it on the response.
pub(crate) async fn request_id(
    State(_state): State<ApiState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_id(value, 64))
        .map_or_else(new_request_id, str::to_owned);
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// Log one authenticated request without recording headers or bodies.
pub async fn record_request(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    request_id: Option<&str>,
    method: &str,
    path: &str,
    status: u16,
    started: Instant,
) {
    let message = format!(
        "method={} route={} status={} duration_ms={}",
        method,
        normalized_route(path),
        status,
        started.elapsed().as_millis().min(60_000)
    );
    let severity = if status >= 500 {
        Severity::Error
    } else if status >= 400 {
        Severity::Warning
    } else {
        Severity::Info
    };
    let _ = record(
        pool,
        &Event {
            severity,
            event: "http.request.completed",
            component: "daemon",
            code: if status < 400 {
                "http_ok"
            } else {
                "http_rejected"
            },
            message: &message,
            request_id,
            job_id: None,
            run_id: None,
            actor_id: Some(&actor.user_id),
        },
    )
    .await;
}

#[derive(Clone, Debug)]
pub struct RequestId(pub String);

fn new_request_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_millis());
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("req-{millis:x}-{sequence:x}")
}

fn normalized_route(path: &str) -> String {
    path.split('/')
        .take(12)
        .map(|part| {
            if part.len() > 48 || part.chars().filter(|value| *value == '-').count() >= 4 {
                "{id}"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn bounded_id(value: Option<&str>, maximum: usize) -> Option<&str> {
    value.filter(|candidate| valid_id(candidate, maximum))
}

fn valid_id(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".:_-".contains(character))
}

/// Remove secrets and complete host paths and enforce the storage bound.
#[must_use]
pub fn sanitize(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let forbidden = [
        "password",
        "passwd",
        "authorization",
        "cookie",
        "csrf",
        "bootstrap",
        "token",
        "secret",
        "private key",
        "postgres://",
        "postgresql://",
        "database_url",
        "-----begin",
    ];
    if forbidden.iter().any(|needle| lower.contains(needle)) {
        return "diagnostic redacted".to_owned();
    }
    let safe = message
        .split_whitespace()
        .map(|word| {
            if word.starts_with('/')
                || word.starts_with("\\\\")
                || word.as_bytes().get(1) == Some(&b':')
            {
                "<path>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    truncate_utf8(&safe, MAX_MESSAGE_BYTES)
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_owned();
    }
    let mut end = maximum.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::{sanitize, valid_id};

    #[test]
    fn secrets_paths_and_oversized_diagnostics_are_bounded() {
        for value in [
            "password=hunter2",
            "Authorization: Bearer abc",
            "Cookie: session=abc",
            "postgresql://user:pass@host/db",
            "-----BEGIN PRIVATE KEY-----",
            "csrf_token=abc",
        ] {
            assert_eq!(sanitize(value), "diagnostic redacted");
        }
        assert_eq!(
            sanitize("failed at /srv/private/file.step"),
            "failed at <path>"
        );
        assert!(sanitize(&"x".repeat(900)).len() <= 512);
        assert!(valid_id("req-safe_1", 64));
        assert!(!valid_id("bad header", 64));
    }
}
