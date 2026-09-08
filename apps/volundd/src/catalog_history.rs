use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};

use crate::api_models::Page;

const TARGET_TYPES: &[&str] = &[
    "model",
    "model-file",
    "source-file",
    "author",
    "tag",
    "collection",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub id: String,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub timestamp_unix_ms: i64,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub outcome: String,
    pub revision: Option<i64>,
    pub summary: Value,
}

/// Load one bounded, target-specific semantic history page.
///
/// # Errors
/// Returns validation or database diagnostics without exposing unapproved metadata.
pub async fn list(
    pool: &PgPool,
    target_type: &str,
    target_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Page<HistoryEvent>, String> {
    if !TARGET_TYPES.contains(&target_type) || target_id.len() > 64 {
        return Err("invalid history target".to_owned());
    }
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT public_id::text,actor_public_id::text,actor_display_name,
         round(extract(epoch FROM occurred_at)*1000)::bigint,action,target_type,
         target_public_id::text,outcome,metadata
         FROM volund.security_audit_events WHERE target_type=$1 AND target_public_id::text=$2
         ORDER BY occurred_at DESC,id DESC LIMIT $3 OFFSET $4",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot load catalog history: {error}"))?;
    let total = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE target_type=$1 AND target_public_id::text=$2",
    )
    .bind(target_type)
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count catalog history: {error}"))?;
    Ok(Page {
        items: rows.iter().map(event).collect(),
        limit,
        offset,
        total,
    })
}

fn event(row: &sqlx::postgres::PgRow) -> HistoryEvent {
    let metadata: Value = row.get(8);
    HistoryEvent {
        id: row.get(0),
        actor_id: row.get(1),
        actor_name: row.get(2),
        timestamp_unix_ms: row.get(3),
        action: row.get(4),
        target_type: row.get(5),
        target_id: row.get(6),
        target_name: metadata
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        outcome: row.get(7),
        revision: metadata.get("revision").and_then(Value::as_i64),
        summary: sanitized(&metadata),
    }
}

fn sanitized(metadata: &Value) -> Value {
    const KEYS: &[&str] = &[
        "fields",
        "revision",
        "planId",
        "code",
        "kind",
        "candidateId",
        "childModelId",
        "collectionId",
        "tagId",
        "sourceTagId",
        "targetTagId",
        "sourceAuthorId",
        "targetAuthorId",
        "modelAction",
        "totalFiles",
        "createdFiles",
        "reusedFiles",
        "relocatedFiles",
        "membershipsRemoved",
        "modelsReassigned",
        "keys",
        "status",
    ];
    let mut result = Map::new();
    if let Some(object) = metadata.as_object() {
        for key in KEYS {
            if let Some(value) = object.get(*key) {
                result.insert((*key).to_owned(), value.clone());
            }
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::sanitized;
    use serde_json::json;

    #[test]
    fn history_drops_paths_confirmation_and_unknown_secrets() {
        assert_eq!(
            sanitized(&json!({"revision":2,"path":"/srv/private","confirmation":"secret"})),
            json!({"revision":2})
        );
    }
}
