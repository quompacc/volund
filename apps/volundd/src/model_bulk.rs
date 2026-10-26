//! Bulk model edit (ADR 0034): one all-or-nothing change applied to many
//! models, each checked against the revision the client saw.

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Extension, State};
use serde::Deserialize;
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_models::ModelSummary;
use crate::model_maintenance::{
    ModelError, audit, persist_author, validate_kind, validate_license,
};
use crate::session::AuthenticatedSession;

pub const MAX_MODELS: usize = 200;
const MAX_IDS: usize = 50;
/// From this many models on, the client must repeat the count as confirmation.
pub const CONFIRM_FROM: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkTarget {
    pub id: String,
    pub expected_revision: i64,
}

/// Fields left out stay unchanged on every model.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkChanges {
    #[serde(default)]
    pub add_tag_ids: Vec<String>,
    #[serde(default)]
    pub remove_tag_ids: Vec<String>,
    #[serde(default)]
    pub add_collection_ids: Vec<String>,
    #[serde(default)]
    pub remove_collection_ids: Vec<String>,
    pub kind: Option<String>,
    /// `Some("")` clears the author; `None` keeps it.
    pub author_name: Option<String>,
    pub license_kind: Option<String>,
    pub license_value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkEditRequest {
    pub models: Vec<BulkTarget>,
    pub changes: BulkChanges,
    pub confirmation: Option<String>,
}

impl BulkChanges {
    fn fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if !self.add_tag_ids.is_empty() || !self.remove_tag_ids.is_empty() {
            fields.push("tags");
        }
        if !self.add_collection_ids.is_empty() || !self.remove_collection_ids.is_empty() {
            fields.push("collections");
        }
        if self.kind.is_some() {
            fields.push("kind");
        }
        if self.author_name.is_some() {
            fields.push("author");
        }
        if self.license_kind.is_some() {
            fields.push("license");
        }
        fields
    }
}

/// The confirmation phrase for `count` models.
#[must_use]
pub fn confirmation_phrase(count: usize) -> String {
    format!("EDIT {count} MODELS")
}

/// A validated license change: kind plus its normalized value.
type LicenseChange = (String, Option<String>);

fn validate(request: &BulkEditRequest) -> Result<Option<LicenseChange>, ModelError> {
    let count = request.models.len();
    if count == 0 || count > MAX_MODELS {
        return Err(ModelError::BadRequest(format!(
            "select 1 to {MAX_MODELS} models"
        )));
    }
    let unique: HashSet<&str> = request
        .models
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    if unique.len() != count {
        return Err(ModelError::BadRequest(
            "each model may appear only once".to_owned(),
        ));
    }
    let changes = &request.changes;
    if changes.fields().is_empty() {
        return Err(ModelError::BadRequest("no change was requested".to_owned()));
    }
    for (name, ids) in [
        ("tag", &changes.add_tag_ids),
        ("tag", &changes.remove_tag_ids),
        ("collection", &changes.add_collection_ids),
        ("collection", &changes.remove_collection_ids),
    ] {
        if ids.len() > MAX_IDS || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
            return Err(ModelError::BadRequest(format!(
                "at most {MAX_IDS} distinct {name} IDs per list"
            )));
        }
    }
    let overlaps = |left: &[String], right: &[String]| left.iter().any(|id| right.contains(id));
    if overlaps(&changes.add_tag_ids, &changes.remove_tag_ids)
        || overlaps(&changes.add_collection_ids, &changes.remove_collection_ids)
    {
        return Err(ModelError::BadRequest(
            "the same tag or collection cannot be added and removed".to_owned(),
        ));
    }
    if let Some(kind) = &changes.kind {
        validate_kind(kind)?;
    }
    let license = match &changes.license_kind {
        Some(kind) => Some((
            kind.clone(),
            validate_license(kind, changes.license_value.as_deref())?,
        )),
        None if changes.license_value.is_some() => {
            return Err(ModelError::BadRequest(
                "licenseValue requires licenseKind".to_owned(),
            ));
        }
        None => None,
    };
    if count >= CONFIRM_FROM && request.confirmation.as_deref() != Some(&confirmation_phrase(count))
    {
        return Err(ModelError::BadRequest(format!(
            "confirm with \"{}\"",
            confirmation_phrase(count)
        )));
    }
    Ok(license)
}

async fn resolve_ids(
    transaction: &mut Transaction<'_, Postgres>,
    table: &str,
    ids: &[String],
) -> Result<Vec<i64>, ModelError> {
    let mut resolved = Vec::with_capacity(ids.len());
    for id in ids {
        let found: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT id FROM volund.{table} WHERE public_id::text=$1 AND active"
        ))
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ModelError::database)?;
        resolved.push(
            found.ok_or_else(|| ModelError::NotFound(format!("{table} entry not found: {id}")))?,
        );
    }
    Ok(resolved)
}

/// Apply one change set to every target model, or to none of them.
///
/// # Errors
/// Returns validation, lookup, revision-conflict (409, naming the models), or
/// database errors; nothing is changed in any error case.
pub(crate) async fn apply(
    state: &ApiState,
    actor: &AuthenticatedSession,
    request: BulkEditRequest,
) -> Result<Vec<ModelSummary>, ModelError> {
    let license = validate(&request)?;
    let mut transaction = state.pool.begin().await.map_err(ModelError::database)?;
    let locked = lock_current(&mut transaction, &request.models).await?;
    let internal: Vec<i64> = locked.iter().map(|(id, _)| *id).collect();
    apply_relations(&mut transaction, &internal, &request.changes).await?;
    let author = match request.changes.author_name.as_deref() {
        Some(name) => Some(persist_author(&mut transaction, actor, Some(name)).await?),
        None => None,
    };
    let batch = format!("{:x}", rand_core::RngCore::next_u64(&mut rand_core::OsRng));
    let fields = request.changes.fields();
    for (model, public_id) in &locked {
        let revision: i64 = sqlx::query_scalar(
            "UPDATE volund.models SET kind=COALESCE($2, kind), \
             author_id=CASE WHEN $3 THEN $4 ELSE author_id END, \
             license_kind=COALESCE($5, license_kind), \
             license_value=CASE WHEN $5 IS NULL THEN license_value ELSE $6 END, \
             updated_at=now(), revision=revision+1 WHERE id=$1 RETURNING revision",
        )
        .bind(model)
        .bind(&request.changes.kind)
        .bind(author.is_some())
        .bind(author.flatten())
        .bind(license.as_ref().map(|(kind, _)| kind))
        .bind(license.as_ref().and_then(|(_, value)| value.clone()))
        .fetch_one(&mut *transaction)
        .await
        .map_err(ModelError::database)?;
        audit(
            &mut transaction,
            actor,
            "model.update",
            public_id,
            json!({"fields": fields, "revision": revision, "bulkBatch": batch, "bulkSize": locked.len()}),
        )
        .await?;
    }
    transaction.commit().await.map_err(ModelError::database)?;
    let mut updated = Vec::with_capacity(request.models.len());
    for target in &request.models {
        if let Some(model) = crate::model_catalog::get_model(&state.pool, &target.id)
            .await
            .map_err(ModelError::Database)?
        {
            updated.push(model);
        }
    }
    Ok(updated)
}

/// Lock every target in id order (no deadlock between concurrent bulk edits)
/// and require each to still be at the revision the client saw.
async fn lock_current(
    transaction: &mut Transaction<'_, Postgres>,
    targets: &[BulkTarget],
) -> Result<Vec<(i64, String)>, ModelError> {
    let ids: Vec<&str> = targets.iter().map(|target| target.id.as_str()).collect();
    let rows = sqlx::query(
        "SELECT id, public_id::text, revision FROM volund.models \
         WHERE public_id::text = ANY($1) AND active ORDER BY id FOR UPDATE",
    )
    .bind(&ids)
    .fetch_all(&mut **transaction)
    .await
    .map_err(ModelError::database)?;
    if rows.len() != targets.len() {
        return Err(ModelError::NotFound(
            "one or more models were not found".to_owned(),
        ));
    }
    let mut stale = Vec::new();
    let mut locked = Vec::with_capacity(rows.len());
    for row in rows {
        let public_id: String = row.get(1);
        let current: i64 = row.get(2);
        if targets
            .iter()
            .any(|target| target.id == public_id && target.expected_revision != current)
        {
            stale.push(public_id.clone());
        }
        locked.push((row.get(0), public_id));
    }
    if !stale.is_empty() {
        return Err(ModelError::Conflict(format!(
            "models changed meanwhile, reload and retry: {}",
            stale.join(", ")
        )));
    }
    Ok(locked)
}

async fn apply_relations(
    transaction: &mut Transaction<'_, Postgres>,
    models: &[i64],
    changes: &BulkChanges,
) -> Result<(), ModelError> {
    let add_tags = resolve_ids(transaction, "tags", &changes.add_tag_ids).await?;
    let remove_tags = resolve_ids(transaction, "tags", &changes.remove_tag_ids).await?;
    let add_collections =
        resolve_ids(transaction, "collections", &changes.add_collection_ids).await?;
    let remove_collections =
        resolve_ids(transaction, "collections", &changes.remove_collection_ids).await?;
    sqlx::query("DELETE FROM volund.model_tags WHERE model_id = ANY($1) AND tag_id = ANY($2)")
        .bind(models)
        .bind(&remove_tags)
        .execute(&mut **transaction)
        .await
        .map_err(ModelError::database)?;
    sqlx::query(
        "INSERT INTO volund.model_tags (model_id, tag_id) SELECT model, tag \
         FROM unnest($1::bigint[]) model, unnest($2::bigint[]) tag ON CONFLICT DO NOTHING",
    )
    .bind(models)
    .bind(&add_tags)
    .execute(&mut **transaction)
    .await
    .map_err(ModelError::database)?;
    sqlx::query(
        "DELETE FROM volund.collection_models WHERE model_id = ANY($1) AND collection_id = ANY($2)",
    )
    .bind(models)
    .bind(&remove_collections)
    .execute(&mut **transaction)
    .await
    .map_err(ModelError::database)?;
    for collection in &add_collections {
        // New members go to the end of each collection, in id order.
        sqlx::query(
            "INSERT INTO volund.collection_models (collection_id, model_id, ordinal) \
             SELECT $1, model, (SELECT COALESCE(max(ordinal) + 1, 0) FROM volund.collection_models \
              WHERE collection_id=$1) + position - 1 \
             FROM unnest($2::bigint[]) WITH ORDINALITY AS selected(model, position) \
             WHERE NOT EXISTS (SELECT 1 FROM volund.collection_models existing \
              WHERE existing.collection_id=$1 AND existing.model_id=selected.model)",
        )
        .bind(collection)
        .bind(models)
        .execute(&mut **transaction)
        .await
        .map_err(ModelError::database)?;
    }
    Ok(())
}

/// `POST /api/v1/models/bulk-edit`.
pub(crate) async fn route(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<BulkEditRequest>,
) -> Result<Json<Vec<ModelSummary>>, ApiError> {
    crate::api_auth::require_catalog_write(&actor)?;
    apply(&state, &actor, request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}
