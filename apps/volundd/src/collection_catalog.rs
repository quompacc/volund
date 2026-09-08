use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api_models::Page;
use crate::import_planner::slugify;
use crate::metadata_normalization::normalize_identity;
use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
pub struct CollectionInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionUpdate {
    pub expected_revision: i64,
    #[serde(flatten)]
    pub input: CollectionInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipUpdate {
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRemoval {
    pub expected_revision: i64,
    pub confirmation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub model_count: i64,
    pub revision: i64,
    pub active: bool,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetail {
    #[serde(flatten)]
    pub collection: CollectionSummary,
    pub model_ids: Vec<String>,
}

#[derive(Debug)]
pub enum CollectionError {
    BadRequest(String),
    NotFound,
    Conflict(String),
    RevisionConflict,
    Database(String),
}

/// List a stable bounded collection page.
///
/// # Errors
/// Returns a database diagnostic when collections cannot be loaded.
pub async fn list(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<Page<CollectionSummary>, String> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT collection.public_id::text,collection.slug,collection.name, \
         collection.description,count(link.model_id)::bigint,collection.revision, \
         collection.active,round(extract(epoch FROM collection.updated_at)*1000)::bigint \
         FROM volund.collections collection LEFT JOIN volund.collection_models link \
         ON link.collection_id=collection.id WHERE collection.active GROUP BY collection.id \
         ORDER BY collection.name,collection.id LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list collections: {error}"))?;
    let total = sqlx::query_scalar("SELECT count(*) FROM volund.collections WHERE active")
        .fetch_one(pool)
        .await
        .map_err(|error| format!("cannot count collections: {error}"))?;
    Ok(Page {
        items: rows.iter().map(collection_from_row).collect(),
        limit,
        offset,
        total,
    })
}

/// Load collection detail and a bounded set of member model IDs.
///
/// # Errors
/// Returns a database diagnostic when detail cannot be loaded.
pub async fn get(pool: &PgPool, id: &str) -> Result<Option<CollectionDetail>, String> {
    let row = sqlx::query(
        "SELECT collection.public_id::text,collection.slug,collection.name, \
         collection.description,count(link.model_id)::bigint,collection.revision, \
         collection.active,round(extract(epoch FROM collection.updated_at)*1000)::bigint \
         FROM volund.collections collection LEFT JOIN volund.collection_models link \
         ON link.collection_id=collection.id WHERE collection.public_id::text=$1 \
         GROUP BY collection.id",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load collection: {error}"))?;
    let Some(row) = row else { return Ok(None) };
    let model_ids = sqlx::query_scalar(
        "SELECT model.public_id::text FROM volund.collection_models member \
         JOIN volund.models model ON model.id=member.model_id \
         JOIN volund.collections collection ON collection.id=member.collection_id \
         WHERE collection.public_id::text=$1 ORDER BY member.ordinal,model.id LIMIT 1000",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot load collection members: {error}"))?;
    Ok(Some(CollectionDetail {
        collection: collection_from_row(&row),
        model_ids,
    }))
}

/// Create a collection through the shared contextual domain.
///
/// # Errors
/// Returns validation, collision, or database errors atomically.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: &CollectionInput,
) -> Result<CollectionSummary, CollectionError> {
    let (name, normalized, description) = validate(input)?;
    let slug = slugify(&name).map_err(CollectionError::BadRequest)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let row = sqlx::query(
        "INSERT INTO volund.collections (slug,name,normalized_name,description) \
         VALUES ($1,$2,$3,$4) RETURNING public_id::text,slug,name,description, \
         0::bigint,revision,active,round(extract(epoch FROM updated_at)*1000)::bigint",
    )
    .bind(slug)
    .bind(name)
    .bind(normalized)
    .bind(description)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    let summary = collection_from_row(&row);
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "collection.create",
        "collection",
        &summary.id,
        json!({"fields":["name","description"],"revision":1}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(summary)
}

/// Update collection metadata using optimistic concurrency.
///
/// # Errors
/// Returns validation, lookup, collision, revision, or database errors atomically.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    request: &CollectionUpdate,
) -> Result<CollectionDetail, CollectionError> {
    let (name, normalized, description) = validate(&request.input)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let current = lock_collection(&mut tx, id).await?;
    ensure_revision(&current, request.expected_revision)?;
    if !current.get::<bool, _>(2) {
        return Err(CollectionError::Conflict(
            "removed collections cannot be edited".to_owned(),
        ));
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.collections SET name=$2,normalized_name=$3,description=$4, \
         revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    )
    .bind(current.get::<i64, _>(0))
    .bind(name)
    .bind(normalized)
    .bind(description)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "collection.update",
        "collection",
        id,
        json!({"fields":["name","description"],"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, id)
        .await
        .map_err(CollectionError::Database)?
        .ok_or(CollectionError::NotFound)
}

/// Add or remove one collection membership without touching the model.
///
/// # Errors
/// Returns lookup, revision, conflict, or database errors atomically.
pub async fn set_membership(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    collection_id: &str,
    model_id: &str,
    expected_revision: i64,
    present: bool,
) -> Result<CollectionDetail, CollectionError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    let collection = lock_collection(&mut tx, collection_id).await?;
    ensure_revision(&collection, expected_revision)?;
    if !collection.get::<bool, _>(2) {
        return Err(CollectionError::Conflict(
            "collection is removed".to_owned(),
        ));
    }
    let model_internal: i64 =
        sqlx::query_scalar("SELECT id FROM volund.models WHERE public_id::text=$1 FOR UPDATE")
            .bind(model_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database_error)?
            .ok_or(CollectionError::NotFound)?;
    let internal_id: i64 = collection.get(0);
    if present {
        sqlx::query(
            "INSERT INTO volund.collection_models (collection_id,model_id,ordinal) \
             VALUES ($1,$2,COALESCE((SELECT max(ordinal)+1 FROM volund.collection_models \
             WHERE collection_id=$1),0)) ON CONFLICT DO NOTHING",
        )
        .bind(internal_id)
        .bind(model_internal)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    } else {
        sqlx::query("DELETE FROM volund.collection_models WHERE collection_id=$1 AND model_id=$2")
            .bind(internal_id)
            .bind(model_internal)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.collections SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    ).bind(internal_id).fetch_one(&mut *tx).await.map_err(database_error)?;
    let model_revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    )
    .bind(model_internal)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        if present {
            "collection.member.add"
        } else {
            "collection.member.remove"
        },
        "collection",
        collection_id,
        json!({"modelId":model_id,"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        if present {
            "model.collection.add"
        } else {
            "model.collection.remove"
        },
        "model",
        model_id,
        json!({"collectionId":collection_id,"revision":model_revision}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, collection_id)
        .await
        .map_err(CollectionError::Database)?
        .ok_or(CollectionError::NotFound)
}

/// Remove a collection vocabulary while preserving every model and source.
///
/// # Errors
/// Returns confirmation, lookup, revision, conflict, or database errors atomically.
pub async fn remove(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    request: &CollectionRemoval,
) -> Result<CollectionDetail, CollectionError> {
    if request.confirmation != format!("REMOVE COLLECTION {id}") {
        crate::security_audit::denied_target(
            pool,
            actor,
            "collection.remove",
            "collection",
            id,
            "confirmation_mismatch",
        )
        .await;
        return Err(CollectionError::BadRequest(
            "exact collection removal confirmation is required".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(database_error)?;
    let current = lock_collection(&mut tx, id).await?;
    ensure_revision(&current, request.expected_revision)?;
    if !current.get::<bool, _>(2) {
        return Err(CollectionError::Conflict(
            "collection is already removed".to_owned(),
        ));
    }
    let affected = sqlx::query(
        "SELECT model.id FROM volund.collection_models link JOIN volund.models model \
         ON model.id=link.model_id WHERE link.collection_id=$1 ORDER BY model.id FOR UPDATE OF model",
    )
    .bind(current.get::<i64, _>(0))
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let model_ids: Vec<i64> = affected.iter().map(|row| row.get(0)).collect();
    let removed = sqlx::query("DELETE FROM volund.collection_models WHERE collection_id=$1")
        .bind(current.get::<i64, _>(0))
        .execute(&mut *tx)
        .await
        .map_err(database_error)?
        .rows_affected();
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.collections SET active=false,removed_at=now(),updated_at=now(), \
         revision=revision+1 WHERE id=$1 RETURNING revision",
    )
    .bind(current.get::<i64, _>(0))
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    let changed_models = sqlx::query(
        "UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=ANY($1) \
         RETURNING public_id::text,revision",
    )
    .bind(&model_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "collection.remove",
        "collection",
        id,
        json!({"membershipsRemoved":removed,"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    for model in changed_models {
        crate::catalog_audit::record(
            &mut tx,
            actor,
            "model.collection.clear",
            "model",
            &model.get::<String, _>(0),
            json!({"collectionId":id,"revision":model.get::<i64, _>(1)}),
        )
        .await
        .map_err(database_error)?;
    }
    tx.commit().await.map_err(database_error)?;
    get(pool, id)
        .await
        .map_err(CollectionError::Database)?
        .ok_or(CollectionError::NotFound)
}

async fn lock_collection(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
) -> Result<sqlx::postgres::PgRow, CollectionError> {
    sqlx::query(
        "SELECT id,revision,active FROM volund.collections WHERE public_id::text=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .ok_or(CollectionError::NotFound)
}

fn ensure_revision(row: &sqlx::postgres::PgRow, expected: i64) -> Result<(), CollectionError> {
    if expected < 1 {
        return Err(CollectionError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    if row.get::<i64, _>(1) == expected {
        Ok(())
    } else {
        Err(CollectionError::RevisionConflict)
    }
}

fn validate(input: &CollectionInput) -> Result<(String, String, String), CollectionError> {
    let (name, normalized) =
        normalize_identity(&input.name, 160).map_err(CollectionError::BadRequest)?;
    let description = input.description.trim().to_owned();
    if description.chars().count() > 4_000 {
        return Err(CollectionError::BadRequest(
            "collection description exceeds 4000 characters".to_owned(),
        ));
    }
    Ok((name, normalized, description))
}

fn collection_from_row(row: &sqlx::postgres::PgRow) -> CollectionSummary {
    CollectionSummary {
        id: row.get(0),
        slug: row.get(1),
        name: row.get(2),
        description: row.get(3),
        model_count: row.get(4),
        revision: row.get(5),
        active: row.get(6),
        updated_at_unix_ms: row.get(7),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn database_error(error: sqlx::Error) -> CollectionError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        CollectionError::Conflict(
            "an active collection with this normalized name already exists".to_owned(),
        )
    } else {
        CollectionError::Database(format!("cannot persist collection: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_metadata_uses_shared_normalization_and_bounds() {
        let valid = CollectionInput {
            name: "  ３D\u{2003}Printers ".to_owned(),
            description: " CoreXY ".to_owned(),
        };
        let (name, normalized, description) = validate(&valid).unwrap();
        assert_eq!(name, "3D Printers");
        assert_eq!(normalized, "3d printers");
        assert_eq!(description, "CoreXY");
    }
}
