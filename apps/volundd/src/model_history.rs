use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};

use crate::api_models::Page;
use crate::model_maintenance::ModelError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelHistoryEvent {
    pub id: String,
    pub actor_display_name: Option<String>,
    pub action: String,
    pub outcome: String,
    pub occurred_at_unix_ms: i64,
    pub change: Value,
}

/// Return one bounded, newest-first model business-history page.
///
/// # Errors
/// Returns lookup or database errors without exposing global security evidence.
pub async fn list(
    pool: &PgPool,
    model_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Page<ModelHistoryEvent>, ModelError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1)")
            .bind(model_id)
            .fetch_one(pool)
            .await
            .map_err(ModelError::database)?;
    if !exists {
        return Err(ModelError::NotFound("model not found".to_owned()));
    }
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT public_id::text,actor_display_name,action,outcome, \
         round(extract(epoch FROM occurred_at)*1000)::bigint,metadata \
         FROM volund.security_audit_events WHERE target_type='model' \
         AND target_public_id::text=$1 AND action LIKE 'model.%' \
         ORDER BY occurred_at DESC,id DESC LIMIT $2 OFFSET $3",
    )
    .bind(model_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(ModelError::database)?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE target_type='model' \
         AND target_public_id::text=$1 AND action LIKE 'model.%'",
    )
    .bind(model_id)
    .fetch_one(pool)
    .await
    .map_err(ModelError::database)?;
    let items = rows
        .iter()
        .map(|row| {
            let action: String = row.get(2);
            let metadata: Value = row.get(5);
            ModelHistoryEvent {
                id: row.get(0),
                actor_display_name: row.get(1),
                action: action.clone(),
                outcome: row.get(3),
                occurred_at_unix_ms: row.get(4),
                change: allowlisted_change(&action, &metadata),
            }
        })
        .collect();
    Ok(Page {
        items,
        limit,
        offset,
        total,
    })
}

fn allowlisted_change(action: &str, metadata: &Value) -> Value {
    let keys: &[&str] = match action {
        "model.create" | "model.update" => &["fields", "revision"],
        "model.thumbnail.update" => &["kind", "candidateId", "revision"],
        "model.problem.status" => &["keys", "status", "revision"],
        "model.component.add" | "model.component.remove" => &["childModelId", "revision"],
        "model.collection.add" | "model.collection.remove" | "model.collection.clear" => {
            &["collectionId", "revision"]
        }
        "model.tag.merge" => &["sourceTagId", "targetTagId", "revision"],
        "model.tag.remove" => &["tagId", "revision"],
        "model.author.merge" => &["sourceAuthorId", "targetAuthorId", "revision"],
        "model.import.commit" => &[
            "modelAction",
            "totalFiles",
            "createdFiles",
            "reusedFiles",
            "relocatedFiles",
            "revision",
        ],
        _ => &[],
    };
    let mut result = Map::new();
    if let Some(source) = metadata.as_object() {
        for key in keys {
            if let Some(value) = source.get(*key) {
                result.insert((*key).to_owned(), value.clone());
            }
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_metadata_is_action_specific_and_drops_sensitive_unknowns() {
        let change = allowlisted_change(
            "model.import.commit",
            &serde_json::json!({
                "createdFiles":2,"revision":4,"confirmation":"secret","path":"/srv/private"
            }),
        );
        assert_eq!(change, serde_json::json!({"createdFiles":2,"revision":4}));
        assert_eq!(
            allowlisted_change("session.login", &serde_json::json!({"code":"x"})),
            serde_json::json!({})
        );
    }
}
