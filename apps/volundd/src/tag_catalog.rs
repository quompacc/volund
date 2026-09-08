use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::api_models::Page;
use crate::import_planner::slugify;
use crate::metadata_normalization::normalize_identity;
use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
pub struct TagInput {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagUpdate {
    pub expected_revision: i64,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagMerge {
    pub expected_revision: i64,
    pub target_tag_id: String,
    pub confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRemoval {
    pub expected_revision: i64,
    pub confirmation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSummary {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub merged_into_id: Option<String>,
    pub model_count: i64,
    pub revision: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug)]
pub enum TagError {
    BadRequest(String),
    NotFound,
    Conflict(String),
    RevisionConflict,
    Database(String),
}

/// List a stable bounded tag page.
///
/// # Errors
/// Returns a database diagnostic when tags cannot be loaded.
pub async fn list(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    include_inactive: bool,
) -> Result<Page<TagSummary>, String> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT tag.public_id::text,tag.name,tag.active,merged.public_id::text, \
         count(model_tag.model_id)::bigint,tag.revision, \
         round(extract(epoch FROM tag.updated_at)*1000)::bigint \
         FROM volund.tags tag LEFT JOIN volund.tags merged ON merged.id=tag.merged_into_id \
         LEFT JOIN volund.model_tags model_tag ON model_tag.tag_id=tag.id \
         WHERE ($3 OR tag.active) GROUP BY tag.id,merged.public_id \
         ORDER BY tag.name,tag.id LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .bind(include_inactive)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list tags: {error}"))?;
    let total = sqlx::query_scalar("SELECT count(*) FROM volund.tags WHERE $1 OR active")
        .bind(include_inactive)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("cannot count tags: {error}"))?;
    Ok(Page {
        items: rows.iter().map(tag_from_row).collect(),
        limit,
        offset,
        total,
    })
}

/// Load one tag, including retained inactive identities.
///
/// # Errors
/// Returns a database diagnostic when the tag cannot be loaded.
pub async fn get(pool: &PgPool, id: &str) -> Result<Option<TagSummary>, String> {
    let row = sqlx::query(
        "SELECT tag.public_id::text,tag.name,tag.active,merged.public_id::text, \
         count(model_tag.model_id)::bigint,tag.revision, \
         round(extract(epoch FROM tag.updated_at)*1000)::bigint \
         FROM volund.tags tag LEFT JOIN volund.tags merged ON merged.id=tag.merged_into_id \
         LEFT JOIN volund.model_tags model_tag ON model_tag.tag_id=tag.id \
         WHERE tag.public_id::text=$1 GROUP BY tag.id,merged.public_id",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load tag: {error}"))?;
    Ok(row.as_ref().map(tag_from_row))
}

/// Create a reusable tag with actor evidence.
///
/// # Errors
/// Returns validation, collision, or database errors atomically.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: &TagInput,
) -> Result<TagSummary, TagError> {
    let (name, normalized) = normalize(&input.name)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let row = sqlx::query(
        "INSERT INTO volund.tags (slug,name,normalized_name) VALUES ($1,$2,$3) \
         RETURNING public_id::text,name,active,NULL::text,0::bigint,revision, \
         round(extract(epoch FROM updated_at)*1000)::bigint",
    )
    .bind(slugify(&name).map_err(TagError::BadRequest)?)
    .bind(name)
    .bind(normalized)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    let tag = tag_from_row(&row);
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "tag.create",
        "tag",
        &tag.id,
        json!({"fields":["name"],"revision":1}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(tag)
}

/// Rename a live tag using optimistic concurrency.
///
/// # Errors
/// Returns validation, lookup, collision, revision, or database errors atomically.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    request: &TagUpdate,
) -> Result<TagSummary, TagError> {
    let (name, normalized) = normalize(&request.name)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let current = lock_tag(&mut tx, id).await?;
    ensure_revision(&current, request.expected_revision)?;
    if !current.get::<bool, _>(2) {
        return Err(TagError::Conflict(
            "inactive tags cannot be renamed".to_owned(),
        ));
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.tags SET name=$2,normalized_name=$3,revision=revision+1,updated_at=now() \
         WHERE id=$1 RETURNING revision",
    )
    .bind(current.get::<i64, _>(0))
    .bind(name)
    .bind(normalized)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "tag.update",
        "tag",
        id,
        json!({"fields":["name"],"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, id)
        .await
        .map_err(TagError::Database)?
        .ok_or(TagError::NotFound)
}

/// Merge a tag and deduplicate every model relationship.
///
/// # Errors
/// Returns confirmation, lookup, conflict, or database errors atomically.
pub async fn merge(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    source_id: &str,
    request: &TagMerge,
) -> Result<TagSummary, TagError> {
    validate_merge_request(pool, actor, source_id, request).await?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let rows = sqlx::query(
        "SELECT id,public_id::text,active,revision FROM volund.tags WHERE public_id::text IN ($1,$2) \
         ORDER BY id FOR UPDATE",
    )
    .bind(source_id)
    .bind(&request.target_tag_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    if rows.len() != 2 {
        return Err(TagError::NotFound);
    }
    let source = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == source_id)
        .ok_or(TagError::NotFound)?;
    let target = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == request.target_tag_id)
        .ok_or(TagError::NotFound)?;
    if !source.get::<bool, _>(2) || !target.get::<bool, _>(2) {
        return Err(TagError::Conflict(
            "only active tags can be merged".to_owned(),
        ));
    }
    if request.expected_revision < 1 {
        return Err(TagError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    if source.get::<i64, _>(3) != request.expected_revision {
        return Err(TagError::RevisionConflict);
    }
    let model_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT model_id FROM volund.model_tags WHERE tag_id=$1 ORDER BY model_id FOR UPDATE",
    )
    .bind(source.get::<i64, _>(0))
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    sqlx::query("INSERT INTO volund.model_tags (model_id,tag_id) SELECT model_id,$2 FROM volund.model_tags WHERE tag_id=$1 ON CONFLICT DO NOTHING")
        .bind(source.get::<i64, _>(0)).bind(target.get::<i64, _>(0)).execute(&mut *tx).await.map_err(database_error)?;
    sqlx::query("DELETE FROM volund.model_tags WHERE tag_id=$1")
        .bind(source.get::<i64, _>(0))
        .execute(&mut *tx)
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
    sqlx::query("UPDATE volund.tags SET active=false,merged_into_id=$2,revision=revision+1,updated_at=now() WHERE id=$1")
        .bind(source.get::<i64, _>(0)).bind(target.get::<i64, _>(0)).execute(&mut *tx).await.map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "tag.merge",
        "tag",
        source_id,
        json!({"targetTagId":request.target_tag_id,"modelsReassigned":model_ids.len()}),
    )
    .await
    .map_err(database_error)?;
    for model in changed_models {
        crate::catalog_audit::record(
            &mut tx,
            actor,
            "model.tag.merge",
            "model",
            &model.get::<String, _>(0),
            json!({"sourceTagId":source_id,"targetTagId":request.target_tag_id,
                "revision":model.get::<i64, _>(1)}),
        )
        .await
        .map_err(database_error)?;
    }
    tx.commit().await.map_err(database_error)?;
    get(pool, &request.target_tag_id)
        .await
        .map_err(TagError::Database)?
        .ok_or(TagError::NotFound)
}

async fn validate_merge_request(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    source_id: &str,
    request: &TagMerge,
) -> Result<(), TagError> {
    if source_id == request.target_tag_id {
        return Err(TagError::BadRequest(
            "a tag cannot be merged into itself".to_owned(),
        ));
    }
    let expected = format!("MERGE TAG {source_id} INTO {}", request.target_tag_id);
    if request.confirmation == expected {
        return Ok(());
    }
    crate::security_audit::denied_target(
        pool,
        actor,
        "tag.merge",
        "tag",
        source_id,
        "confirmation_mismatch",
    )
    .await;
    Err(TagError::BadRequest(
        "exact tag merge confirmation is required".to_owned(),
    ))
}

/// Remove a tag relationship vocabulary without deleting models or files.
///
/// # Errors
/// Returns confirmation, lookup, revision, conflict, or database errors atomically.
pub async fn remove(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    request: &TagRemoval,
) -> Result<TagSummary, TagError> {
    if request.confirmation != format!("REMOVE TAG {id}") {
        crate::security_audit::denied_target(
            pool,
            actor,
            "tag.remove",
            "tag",
            id,
            "confirmation_mismatch",
        )
        .await;
        return Err(TagError::BadRequest(
            "exact tag removal confirmation is required".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(database_error)?;
    let current = lock_tag(&mut tx, id).await?;
    ensure_revision(&current, request.expected_revision)?;
    if !current.get::<bool, _>(2) {
        return Err(TagError::Conflict("tag is already inactive".to_owned()));
    }
    let model_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT model_id FROM volund.model_tags WHERE tag_id=$1 ORDER BY model_id FOR UPDATE",
    )
    .bind(current.get::<i64, _>(0))
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    sqlx::query("DELETE FROM volund.model_tags WHERE tag_id=$1")
        .bind(current.get::<i64, _>(0))
        .execute(&mut *tx)
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
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.tags SET active=false,removed_at=now(),revision=revision+1,updated_at=now() \
         WHERE id=$1 RETURNING revision",
    )
    .bind(current.get::<i64, _>(0))
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    for model in changed_models {
        crate::catalog_audit::record(
            &mut tx,
            actor,
            "model.tag.remove",
            "model",
            &model.get::<String, _>(0),
            json!({"tagId":id,"revision":model.get::<i64, _>(1)}),
        )
        .await
        .map_err(database_error)?;
    }
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "tag.remove",
        "tag",
        id,
        json!({"membershipsRemoved":model_ids.len(),"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, id)
        .await
        .map_err(TagError::Database)?
        .ok_or(TagError::NotFound)
}

/// Replace model tag memberships using the same normalized identities as administration.
///
/// # Errors
/// Returns validation, collision, or database errors without partial membership changes.
pub async fn replace_model_tags(
    tx: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    model_id: i64,
    names: &[String],
    public_ids: Option<&[String]>,
) -> Result<(), TagError> {
    sqlx::query("DELETE FROM volund.model_tags WHERE model_id=$1")
        .bind(model_id)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    if let Some(public_ids) = public_ids {
        let mut unique = std::collections::BTreeSet::new();
        for public_id in public_ids {
            if !unique.insert(public_id) {
                continue;
            }
            let tag_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM volund.tags WHERE public_id::text=$1 AND active FOR UPDATE",
            )
            .bind(public_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(database_error)?;
            let tag_id = tag_id.ok_or(TagError::NotFound)?;
            sqlx::query("INSERT INTO volund.model_tags (model_id,tag_id) VALUES ($1,$2)")
                .bind(model_id)
                .bind(tag_id)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
        }
        return Ok(());
    }
    for name in names {
        let (tag_id, created) = resolve_or_create(tx, name).await?;
        sqlx::query(
            "INSERT INTO volund.model_tags (model_id,tag_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(model_id)
        .bind(tag_id)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
        if let Some(public_id) = created {
            crate::catalog_audit::record(
                tx,
                actor,
                "tag.create",
                "tag",
                &public_id,
                json!({"fields":["name"],"context":"model-maintenance","revision":1}),
            )
            .await
            .map_err(database_error)?;
        }
    }
    Ok(())
}

/// Resolve or create normalized tags for atomic import publication.
///
/// # Errors
/// Returns validation, collision, or database errors.
pub async fn replace_import_tags(
    tx: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    model_id: i64,
    names: &[String],
) -> Result<(), TagError> {
    sqlx::query("DELETE FROM volund.model_tags WHERE model_id=$1")
        .bind(model_id)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    for name in names {
        let (tag_id, created) = resolve_or_create(tx, name).await?;
        sqlx::query(
            "INSERT INTO volund.model_tags (model_id,tag_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(model_id)
        .bind(tag_id)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
        if let Some(public_id) = created {
            crate::catalog_audit::record(
                tx,
                actor,
                "tag.create",
                "tag",
                &public_id,
                json!({"fields":["name"],"context":"import","revision":1}),
            )
            .await
            .map_err(database_error)?;
        }
    }
    Ok(())
}

async fn resolve_or_create(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
) -> Result<(i64, Option<String>), TagError> {
    let (name, normalized) = normalize(name)?;
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM volund.tags WHERE normalized_name=$1 AND active FOR UPDATE",
    )
    .bind(&normalized)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    {
        return Ok((id, None));
    }
    let row = sqlx::query("INSERT INTO volund.tags (slug,name,normalized_name) VALUES ($1,$2,$3) RETURNING id,public_id::text")
        .bind(slugify(&name).map_err(TagError::BadRequest)?).bind(name).bind(normalized)
        .fetch_one(&mut **tx).await.map_err(database_error)?;
    Ok((row.get(0), Some(row.get(1))))
}

async fn lock_tag(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<sqlx::postgres::PgRow, TagError> {
    sqlx::query("SELECT id,revision,active FROM volund.tags WHERE public_id::text=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?
        .ok_or(TagError::NotFound)
}

fn ensure_revision(row: &sqlx::postgres::PgRow, expected: i64) -> Result<(), TagError> {
    if expected < 1 {
        return Err(TagError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    if row.get::<i64, _>(1) == expected {
        Ok(())
    } else {
        Err(TagError::RevisionConflict)
    }
}

fn normalize(name: &str) -> Result<(String, String), TagError> {
    normalize_identity(name, 50).map_err(TagError::BadRequest)
}

fn tag_from_row(row: &sqlx::postgres::PgRow) -> TagSummary {
    TagSummary {
        id: row.get(0),
        name: row.get(1),
        active: row.get(2),
        merged_into_id: row.get(3),
        model_count: row.get(4),
        revision: row.get(5),
        updated_at_unix_ms: row.get(6),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn database_error(error: sqlx::Error) -> TagError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        TagError::Conflict("an active tag with this normalized name already exists".to_owned())
    } else {
        TagError::Database(format!("cannot persist tag: {error}"))
    }
}

#[cfg(test)]
#[path = "tag_catalog_tests.rs"]
mod tests;
