use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use std::path::Path;

use crate::model_maintenance::ModelError;
use crate::session::AuthenticatedSession;

const MAX_PROBLEMS: usize = 500;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProblem {
    pub key: String,
    pub severity: String,
    pub status: String,
    pub code: String,
    pub message: String,
    pub source_id: String,
    pub source_name: String,
    pub source_url: String,
    pub diagnostics_url: Option<String>,
    pub preview_id: String,
    pub profile: String,
    pub occurred_at_unix_ms: i64,
    pub remediation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProblemStatusUpdate {
    pub expected_revision: i64,
    pub keys: Vec<String>,
    pub status: String,
}

/// List bounded conversion problems belonging to one active model.
///
/// # Errors
/// Returns lookup or database errors without exposing host paths or raw diagnostics.
pub async fn list(
    pool: &PgPool,
    derived_root: &Path,
    model_id: &str,
) -> Result<Vec<ModelProblem>, ModelError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1 AND active)",
    )
    .bind(model_id)
    .fetch_one(pool)
    .await
    .map_err(ModelError::database)?;
    if !exists {
        return Err(ModelError::NotFound("model not found".to_owned()));
    }
    let rows = sqlx::query(
        "SELECT run.public_id::text,source.public_id::text,source.relative_path,run.status,
         run.profile,round(extract(epoch FROM run.finished_at)*1000)::bigint,run.diagnostics,
         (SELECT artifact.public_id::text FROM volund.derived_artifacts artifact
          WHERE artifact.conversion_run_id=run.id AND artifact.artifact_kind='diagnostics'
          ORDER BY artifact.id DESC LIMIT 1),assembly.public_id::text,
         assembly.relative_path,assembly.byte_size
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id
         JOIN volund.source_files source ON source.id=link.source_file_id
         JOIN LATERAL (
          SELECT DISTINCT ON (candidate.profile) candidate.*
          FROM volund.conversion_runs candidate
          WHERE candidate.content_object_id=source.content_object_id
          ORDER BY candidate.profile,candidate.id DESC
         ) run ON true
         LEFT JOIN LATERAL (
          SELECT artifact.public_id,artifact.relative_path,artifact.byte_size
          FROM volund.derived_artifacts artifact
          WHERE artifact.conversion_run_id=run.id AND artifact.artifact_kind='assembly-manifest'
          ORDER BY artifact.id DESC LIMIT 1
         ) assembly ON true
         WHERE model.public_id::text=$1
          AND (run.diagnostics <> '[]'::jsonb OR assembly.public_id IS NOT NULL)
         ORDER BY link.is_primary DESC,run.finished_at DESC NULLS LAST,run.id DESC LIMIT 200",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(ModelError::database)?;
    let states = states(pool, model_id).await?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        append_row(derived_root, row, &states, &mut seen, &mut result).await;
        if result.len() >= MAX_PROBLEMS {
            break;
        }
    }
    Ok(result)
}

async fn append_row(
    derived_root: &Path,
    row: PgRow,
    states: &HashMap<String, String>,
    seen: &mut HashSet<String>,
    result: &mut Vec<ModelProblem>,
) {
    let preview_id: String = row.get(0);
    let source_id: String = row.get(1);
    let source_name = bounded_text(row.get::<String, _>(2), 500);
    let profile = bounded_text(row.get(4), 80);
    let occurred_at_unix_ms = row.get::<Option<i64>, _>(5).unwrap_or(0);
    let diagnostics: Value = row.get(6);
    for (index, item) in diagnostics
        .as_array()
        .into_iter()
        .flatten()
        .take(50)
        .enumerate()
    {
        if result.len() >= MAX_PROBLEMS {
            return;
        }
        let key = format!("{preview_id}:{index}");
        if !seen.insert(key.clone()) {
            continue;
        }
        let run_status: String = row.get(3);
        let severity = bounded_field(item, "severity", 16)
            .filter(|value| matches!(value.as_str(), "info" | "warning" | "error"))
            .unwrap_or_else(|| {
                if run_status == "failed" {
                    "error"
                } else {
                    "warning"
                }
                .to_owned()
            });
        result.push(ModelProblem {
            status: states
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "open".to_owned()),
            key,
            severity,
            code: bounded_field(item, "code", 80).unwrap_or_else(|| "conversion.failed".to_owned()),
            message: bounded_field(item, "message", 1000).map_or_else(
                || "Konvertierungsdiagnose ohne Beschreibung".to_owned(),
                |value| crate::operational_log::sanitize(&value),
            ),
            source_url: format!("/api/v1/files/{source_id}/content?download=true"),
            diagnostics_url: row
                .get::<Option<String>, _>(7)
                .map(|id| format!("/api/v1/artifacts/{id}/content")),
            source_id: source_id.clone(),
            source_name: source_name.clone(),
            preview_id: preview_id.clone(),
            profile: profile.clone(),
            occurred_at_unix_ms,
            remediation: "Original prüfen, Profil anpassen und Vorschau erneut erzeugen."
                .to_owned(),
        });
    }
    let assembly = (
        row.get::<Option<String>, _>(8),
        row.get::<Option<String>, _>(9),
        row.get::<Option<i64>, _>(10),
    );
    let (Some(artifact_id), Some(path), Some(size)) = assembly else {
        return;
    };
    let Some(message) = crate::assembly_manifest::inspect(derived_root, &path, size).await else {
        return;
    };
    let key = format!("{preview_id}:assembly");
    if result.len() >= MAX_PROBLEMS || !seen.insert(key.clone()) {
        return;
    }
    result.push(ModelProblem {
        status: states
            .get(&key)
            .cloned()
            .unwrap_or_else(|| "open".to_owned()),
        key,
        severity: "error".to_owned(),
        code: "assembly.manifest.invalid".to_owned(),
        message,
        source_url: format!("/api/v1/files/{source_id}/content?download=true"),
        diagnostics_url: Some(format!("/api/v1/artifacts/{artifact_id}/content")),
        source_id,
        source_name,
        preview_id,
        profile,
        occurred_at_unix_ms,
        remediation: "STEP-Vorschau mit dem aktuellen Konverter neu erzeugen.".to_owned(),
    });
}

/// Append a revision-bound durable ignore, resolve, or reopen decision.
///
/// # Errors
/// Returns validation, stale-revision, lookup, or database errors atomically.
pub async fn set_status(
    pool: &PgPool,
    derived_root: &Path,
    actor: &AuthenticatedSession,
    model_id: &str,
    request: &ProblemStatusUpdate,
) -> Result<Vec<ModelProblem>, ModelError> {
    if request.expected_revision < 1
        || !matches!(request.status.as_str(), "open" | "ignored" | "resolved")
        || request.keys.is_empty()
        || request.keys.len() > 100
    {
        return Err(ModelError::BadRequest(
            "invalid bounded problem status update".to_owned(),
        ));
    }
    let unique: HashSet<&str> = request.keys.iter().map(String::as_str).collect();
    if unique.len() != request.keys.len() || request.keys.iter().any(|key| key.len() > 80) {
        return Err(ModelError::BadRequest(
            "problem keys must be unique and bounded".to_owned(),
        ));
    }
    let known: HashSet<String> = list(pool, derived_root, model_id)
        .await?
        .into_iter()
        .map(|item| item.key)
        .collect();
    if request.keys.iter().any(|key| !known.contains(key)) {
        return Err(ModelError::BadRequest(
            "problem does not belong to this model".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(ModelError::database)?;
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM volund.models WHERE public_id::text=$1 AND active FOR UPDATE",
    )
    .bind(model_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ModelError::database)?
    .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    if revision != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "model.problem.status",
        "model",
        model_id,
        json!({"keys":request.keys,"status":request.status,"revision":revision}),
    )
    .await
    .map_err(ModelError::database)?;
    tx.commit().await.map_err(ModelError::database)?;
    list(pool, derived_root, model_id).await
}

async fn states(pool: &PgPool, model_id: &str) -> Result<HashMap<String, String>, ModelError> {
    let rows = sqlx::query(
        "SELECT metadata FROM volund.security_audit_events WHERE target_type='model'
         AND target_public_id::text=$1 AND action='model.problem.status'
         ORDER BY occurred_at DESC,id DESC LIMIT 500",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(ModelError::database)?;
    let mut result = HashMap::new();
    for row in rows {
        let metadata: Value = row.get(0);
        let Some(status) = metadata.get("status").and_then(Value::as_str) else {
            continue;
        };
        let Some(keys) = metadata.get("keys").and_then(Value::as_array) else {
            continue;
        };
        for key in keys.iter().filter_map(Value::as_str) {
            result
                .entry(key.to_owned())
                .or_insert_with(|| status.to_owned());
        }
    }
    Ok(result)
}

fn bounded_field(value: &Value, key: &str, limit: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| bounded_text(text.to_owned(), limit))
}

fn bounded_text(mut value: String, limit: usize) -> String {
    if value.chars().count() > limit {
        value = value.chars().take(limit).collect();
    }
    value
}
