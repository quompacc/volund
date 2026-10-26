//! Catalog-wide problem overview (ADR 0033): every model's conversion problems
//! plus missing originals and failed library scans, in one bounded list.

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::path::Path;

use crate::api::ApiState;
use crate::api_error::ApiError;

/// Models whose problems one request inspects; the response says when more exist.
const MAX_MODELS: i64 = 2000;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewQuery {
    /// Minimum severity: `info`, `warning` (default), or `error`.
    pub severity: Option<String>,
    /// `open` (default), `ignored`, `resolved`, or `all`.
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewItem {
    pub key: String,
    /// `model` or `library`.
    pub scope: &'static str,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub library_key: Option<String>,
    pub severity: String,
    pub status: String,
    pub code: String,
    pub message: String,
    pub source_name: Option<String>,
    pub occurred_at_unix_ms: i64,
    pub remediation: String,
}

#[derive(Debug, Default, Serialize)]
pub struct SeverityCounts {
    pub error: i64,
    pub warning: i64,
    pub info: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewPage {
    pub items: Vec<OverviewItem>,
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    /// Per-severity totals for the status filter, independent of `severity`.
    pub counts: SeverityCounts,
    /// More models had candidate problems than one request inspects.
    pub truncated: bool,
}

pub(crate) fn rank(severity: &str) -> u8 {
    match severity {
        "error" => 3,
        "warning" => 2,
        _ => 1,
    }
}

struct Filter {
    minimum: u8,
    status: Option<String>,
    limit: i64,
    offset: i64,
}

fn parse(query: &OverviewQuery) -> Result<Filter, String> {
    let minimum = match query.severity.as_deref() {
        None | Some("" | "warning") => 2,
        Some("info") => 1,
        Some("error") => 3,
        Some(_) => return Err("severity must be info, warning, or error".to_owned()),
    };
    let status = match query.status.as_deref() {
        None | Some("" | "open") => Some("open".to_owned()),
        Some("all") => None,
        Some(value @ ("ignored" | "resolved")) => Some(value.to_owned()),
        Some(_) => return Err("status must be open, ignored, resolved, or all".to_owned()),
    };
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    if !(1..=MAX_LIMIT).contains(&limit) || offset < 0 {
        return Err(format!(
            "limit must be 1 to {MAX_LIMIT} and offset non-negative"
        ));
    }
    Ok(Filter {
        minimum,
        status,
        limit,
        offset,
    })
}

/// `GET /api/v1/problems`.
pub(crate) async fn route(
    State(state): State<ApiState>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<OverviewPage>, ApiError> {
    let filter = parse(&query).map_err(ApiError::BadRequest)?;
    overview(&state.pool, &state.derived_root, &filter)
        .await
        .map(Json)
        .map_err(ApiError::Database)
}

async fn overview(
    pool: &PgPool,
    derived_root: &Path,
    filter: &Filter,
) -> Result<OverviewPage, String> {
    let (mut all, truncated) = model_problems(pool, derived_root).await?;
    all.extend(missing_originals(pool).await?);
    all.extend(failed_scans(pool).await?);
    all.retain(|item| {
        filter
            .status
            .as_ref()
            .is_none_or(|status| &item.status == status)
    });
    let mut counts = SeverityCounts::default();
    for item in &all {
        match item.severity.as_str() {
            "error" => counts.error += 1,
            "warning" => counts.warning += 1,
            _ => counts.info += 1,
        }
    }
    all.retain(|item| rank(&item.severity) >= filter.minimum);
    all.sort_by(|left, right| {
        rank(&right.severity)
            .cmp(&rank(&left.severity))
            .then(right.occurred_at_unix_ms.cmp(&left.occurred_at_unix_ms))
            .then(left.key.cmp(&right.key))
    });
    let total = i64::try_from(all.len()).map_err(|_| "problem count overflow".to_owned())?;
    let items = all
        .into_iter()
        .skip(usize::try_from(filter.offset).unwrap_or(usize::MAX))
        .take(usize::try_from(filter.limit).unwrap_or(0))
        .collect();
    Ok(OverviewPage {
        items,
        limit: filter.limit,
        offset: filter.offset,
        total,
        counts,
        truncated,
    })
}

/// Conversion and assembly problems of every active model that has a
/// candidate: a latest run with diagnostics or an assembly manifest.
async fn model_problems(
    pool: &PgPool,
    derived_root: &Path,
) -> Result<(Vec<OverviewItem>, bool), String> {
    let models: Vec<(String, String)> = sqlx::query_as(
        "SELECT model.public_id::text, model.name FROM volund.models model WHERE model.active \
         AND EXISTS (SELECT 1 FROM volund.model_source_files link \
          JOIN volund.source_files source ON source.id=link.source_file_id \
          JOIN volund.conversion_runs run ON run.content_object_id=source.content_object_id \
          WHERE link.model_id=model.id AND (run.diagnostics <> '[]'::jsonb OR EXISTS \
           (SELECT 1 FROM volund.derived_artifacts artifact WHERE artifact.conversion_run_id=run.id \
            AND artifact.artifact_kind='assembly-manifest'))) \
         ORDER BY model.updated_at DESC, model.id LIMIT $1",
    )
    .bind(MAX_MODELS + 1)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list problem candidates: {error}"))?;
    let truncated = i64::try_from(models.len()).unwrap_or(i64::MAX) > MAX_MODELS;
    let mut items = Vec::new();
    for (model_id, model_name) in models
        .into_iter()
        .take(usize::try_from(MAX_MODELS).unwrap_or(0))
    {
        let problems = crate::problem_catalog::list(pool, derived_root, &model_id)
            .await
            .map_err(|error| format!("cannot list model problems: {error:?}"))?;
        items.extend(problems.into_iter().map(|problem| OverviewItem {
            key: format!("{model_id}:{}", problem.key),
            scope: "model",
            model_id: Some(model_id.clone()),
            model_name: Some(model_name.clone()),
            library_key: None,
            severity: problem.severity,
            status: problem.status,
            code: problem.code,
            message: problem.message,
            source_name: Some(problem.source_name),
            occurred_at_unix_ms: problem.occurred_at_unix_ms,
            remediation: problem.remediation,
        }));
    }
    Ok((items, truncated))
}

/// Originals linked to an active model that are missing or not available.
async fn missing_originals(pool: &PgPool) -> Result<Vec<OverviewItem>, String> {
    let rows = sqlx::query(
        "SELECT model.public_id::text, model.name, source.public_id::text, source.relative_path, \
         root.root_key, source.lifecycle_state, \
         round(extract(epoch FROM COALESCE(source.missing_at, model.updated_at))*1000)::bigint \
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.library_roots root ON root.id=source.library_root_id \
         WHERE model.active AND (source.missing_at IS NOT NULL OR source.lifecycle_state<>'available') \
         ORDER BY model.id, source.id LIMIT 5000",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list missing originals: {error}"))?;
    Ok(rows
        .iter()
        .map(|row| {
            let state: String = row.get(5);
            let path: String = row.get(3);
            let (code, message, remediation) = if state == "available" {
                (
                    "source.missing",
                    format!("Originaldatei fehlt in der Bibliothek: {path}"),
                    "Datei wiederherstellen oder verschieben, dann die Bibliothek neu scannen.",
                )
            } else {
                (
                    "source.unavailable",
                    format!("Originaldatei ist nicht verfügbar ({state}): {path}"),
                    "Status der Datei unter Administration prüfen oder die Verknüpfung entfernen.",
                )
            };
            OverviewItem {
                key: format!(
                    "missing:{}:{}",
                    row.get::<String, _>(0),
                    row.get::<String, _>(2)
                ),
                scope: "model",
                model_id: Some(row.get(0)),
                model_name: Some(row.get(1)),
                library_key: Some(row.get(4)),
                severity: "error".to_owned(),
                status: "open".to_owned(),
                code: code.to_owned(),
                message,
                source_name: Some(path),
                occurred_at_unix_ms: row.get(6),
                remediation: remediation.to_owned(),
            }
        })
        .collect())
}

/// Libraries whose most recent scan failed.
async fn failed_scans(pool: &PgPool) -> Result<Vec<OverviewItem>, String> {
    let rows = sqlx::query(
        "SELECT root.root_key, root.display_name, latest.public_id::text, latest.error_message, \
         round(extract(epoch FROM COALESCE(latest.finished_at, latest.started_at))*1000)::bigint \
         FROM volund.library_roots root JOIN LATERAL (SELECT * FROM volund.scan_runs scan \
          WHERE scan.library_root_id=root.id AND scan.status IN ('completed','failed') \
          ORDER BY scan.id DESC LIMIT 1) latest ON true \
         WHERE latest.status='failed' ORDER BY root.root_key",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list failed scans: {error}"))?;
    Ok(rows
        .iter()
        .map(|row| OverviewItem {
            key: format!("scan:{}", row.get::<String, _>(2)),
            scope: "library",
            model_id: None,
            model_name: None,
            library_key: Some(row.get(0)),
            severity: "error".to_owned(),
            status: "open".to_owned(),
            code: "scan.failed".to_owned(),
            message: format!(
                "Letzter Scan der Bibliothek „{}“ ist fehlgeschlagen: {}",
                row.get::<String, _>(1),
                crate::operational_log::sanitize(
                    &row.get::<Option<String>, _>(3)
                        .unwrap_or_else(|| "ohne Meldung".to_owned())
                )
            ),
            source_name: None,
            occurred_at_unix_ms: row.get(4),
            remediation: "Ursache beheben und unter Administration → Bibliotheken neu scannen."
                .to_owned(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{OverviewQuery, parse, rank};

    #[test]
    fn overview_filters_are_validated_with_safe_defaults() {
        let filter = parse(&OverviewQuery::default()).unwrap();
        assert_eq!(
            (filter.minimum, filter.status.as_deref(), filter.limit),
            (2, Some("open"), 50)
        );
        assert!(
            parse(&OverviewQuery {
                status: Some("all".into()),
                ..OverviewQuery::default()
            })
            .unwrap()
            .status
            .is_none()
        );
        for invalid in [
            OverviewQuery {
                severity: Some("fatal".into()),
                ..OverviewQuery::default()
            },
            OverviewQuery {
                status: Some("closed".into()),
                ..OverviewQuery::default()
            },
            OverviewQuery {
                limit: Some(0),
                ..OverviewQuery::default()
            },
            OverviewQuery {
                offset: Some(-1),
                ..OverviewQuery::default()
            },
        ] {
            assert!(parse(&invalid).is_err());
        }
        assert!(rank("error") > rank("warning") && rank("warning") > rank("info"));
    }
}
