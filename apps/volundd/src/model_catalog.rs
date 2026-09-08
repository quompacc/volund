use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api_models::{
    ModelComponentSummary, ModelFileSummary, ModelSummary, Page, ThumbnailState,
};
use crate::model_maintenance::ModelError;
use crate::session::AuthenticatedSession;

const MODEL_SELECT: &str = "SELECT model.public_id::text,model.slug,model.name,model.description,model.kind, \
     model.license_kind,model.license_value,author.name, \
     (SELECT source.public_id::text FROM volund.model_source_files primary_link \
       JOIN volund.source_files source ON source.id=primary_link.source_file_id \
       WHERE primary_link.model_id=model.id AND primary_link.is_primary), \
     count(link.source_file_id)::bigint, \
     COALESCE(array_agg(DISTINCT content.detected_format) FILTER \
       (WHERE content.detected_format IS NOT NULL),ARRAY[]::text[]), \
     round(extract(epoch FROM model.updated_at)*1000)::bigint, \
     COALESCE((SELECT array_agg(tag.name ORDER BY tag.name,tag.id) \
       FROM volund.model_tags model_tag JOIN volund.tags tag ON tag.id=model_tag.tag_id \
       WHERE model_tag.model_id=model.id),ARRAY[]::text[]), \
     COALESCE((SELECT array_agg(collection.name ORDER BY collection.name,collection.id) \
       FROM volund.collection_models collection_model JOIN volund.collections collection \
       ON collection.id=collection_model.collection_id \
       WHERE collection_model.model_id=model.id),ARRAY[]::text[]), \
     model.viewer_rotation_x,model.viewer_rotation_y,model.viewer_rotation_z,model.revision, \
     model.thumbnail_kind, \
     (SELECT source.public_id::text FROM volund.source_files source WHERE source.id=model.thumbnail_source_file_id), \
     (SELECT source.missing_at IS NOT NULL FROM volund.source_files source WHERE source.id=model.thumbnail_source_file_id), \
     (SELECT artifact.public_id::text FROM volund.derived_artifacts artifact WHERE artifact.id=model.thumbnail_artifact_id), \
     (SELECT run.status='ready' FROM volund.derived_artifacts artifact JOIN volund.conversion_runs run \
       ON run.id=artifact.conversion_run_id WHERE artifact.id=model.thumbnail_artifact_id), \
     COALESCE((SELECT array_agg(tag.public_id::text ORDER BY tag.name,tag.id) \
       FROM volund.model_tags model_tag JOIN volund.tags tag ON tag.id=model_tag.tag_id \
       WHERE model_tag.model_id=model.id),ARRAY[]::text[]), \
     (SELECT artifact.public_id::text FROM volund.model_source_files fallback_link \
       JOIN volund.source_files fallback_source ON fallback_source.id=fallback_link.source_file_id \
       JOIN volund.conversion_runs fallback_run ON fallback_run.content_object_id=fallback_source.content_object_id \
       JOIN volund.derived_artifacts artifact ON artifact.conversion_run_id=fallback_run.id \
       WHERE fallback_link.model_id=model.id AND fallback_source.lifecycle_state='available' \
       AND fallback_source.missing_at IS NULL AND fallback_run.status='ready' \
       AND artifact.artifact_kind='thumbnail-raster' \
       ORDER BY fallback_link.is_primary DESC,fallback_run.finished_at DESC NULLS LAST,artifact.id DESC LIMIT 1) \
     FROM volund.models model LEFT JOIN volund.authors author ON author.id=model.author_id \
     LEFT JOIN volund.model_source_files link ON link.model_id=model.id \
     LEFT JOIN volund.source_files source ON source.id=link.source_file_id \
     LEFT JOIN volund.content_objects content ON content.id=source.content_object_id";

/// List persistent models with path-independent file aggregates.
///
/// # Errors
/// Returns a database diagnostic when the catalog cannot be loaded.
pub async fn list_models(pool: &PgPool) -> Result<Vec<ModelSummary>, String> {
    let query = format!(
        "{MODEL_SELECT} WHERE model.active GROUP BY model.id,author.name ORDER BY model.updated_at DESC,model.name,model.id"
    );
    let rows = sqlx::query(&query)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("cannot list models: {error}"))?;
    Ok(rows.iter().map(model_from_row).collect())
}

/// Load the unique detail source for a model page and editor.
///
/// # Errors
/// Returns a database diagnostic when the model cannot be loaded.
pub async fn get_model(pool: &PgPool, model_id: &str) -> Result<Option<ModelSummary>, String> {
    let query = format!(
        "{MODEL_SELECT} WHERE model.public_id::text=$1 AND model.active GROUP BY model.id,author.name"
    );
    sqlx::query(&query)
        .bind(model_id)
        .fetch_optional(pool)
        .await
        .map(|row| row.as_ref().map(model_from_row))
        .map_err(|error| format!("cannot load model detail: {error}"))
}

/// List every stable source-file relationship for one model.
///
/// # Errors
/// Returns a database diagnostic when the model or files cannot be loaded.
pub async fn list_model_files(
    pool: &PgPool,
    model_id: &str,
) -> Result<Option<Vec<ModelFileSummary>>, String> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1 AND active)",
    )
    .bind(model_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot resolve model: {error}"))?;
    if !exists {
        return Ok(None);
    }
    let rows = sqlx::query(
        "SELECT source.public_id::text,source.relative_path,root.root_key,root.display_name, \
         link.role,link.is_primary,content.detected_format,content.byte_size, \
         (extract(epoch FROM source.filesystem_modified_at)*1000)::bigint, \
         source.missing_at IS NOT NULL,link.revision,link.caption,link.description,link.notes, \
         link.printable,link.printed,link.pre_supported,link.up_axis,link.support_hint, \
         link.orientation_x,link.orientation_y,link.orientation_z,source.lifecycle_state, \
         source.lifecycle_revision FROM volund.models model \
         JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.library_roots root ON root.id=source.library_root_id \
         JOIN volund.content_objects content ON content.id=source.content_object_id \
         WHERE model.public_id::text=$1 AND model.active AND source.lifecycle_state='available' \
         ORDER BY link.is_primary DESC,link.ordinal,source.relative_path",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list model files: {error}"))?;
    Ok(Some(
        rows.iter()
            .map(|row| ModelFileSummary {
                id: row.get(0),
                path: row.get(1),
                root_key: row.get(2),
                root_name: row.get(3),
                role: row.get(4),
                primary: row.get(5),
                format: row.get(6),
                byte_size: row.get(7),
                modified_at_unix_ms: row.get(8),
                missing: row.get(9),
                revision: row.get(10),
                caption: row.get(11),
                description: row.get(12),
                notes: row.get(13),
                printable: row.get(14),
                printed: row.get(15),
                pre_supported: row.get(16),
                up_axis: row.get(17),
                support_hint: row.get(18),
                orientation: [row.get(19), row.get(20), row.get(21)],
                lifecycle_state: row.get(22),
                lifecycle_revision: row.get(23),
            })
            .collect(),
    ))
}

/// Load one bounded page without materializing the complete model fileset.
///
/// # Errors
/// Returns a database diagnostic when the model or page cannot be loaded.
pub async fn list_model_files_page(
    pool: &PgPool,
    model_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Option<Page<ModelFileSummary>>, String> {
    let total: Option<i64> = sqlx::query_scalar(
        "SELECT count(link.source_file_id)::bigint FROM volund.models model \
         LEFT JOIN volund.model_source_files link ON link.model_id=model.id \
         WHERE model.public_id::text=$1 AND model.active GROUP BY model.id",
    )
    .bind(model_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot count model files: {error}"))?;
    let Some(total) = total else { return Ok(None) };
    let rows = sqlx::query(
        "SELECT source.public_id::text,source.relative_path,root.root_key,root.display_name, \
         link.role,link.is_primary,content.detected_format,content.byte_size, \
         (extract(epoch FROM source.filesystem_modified_at)*1000)::bigint, \
         source.missing_at IS NOT NULL,link.revision,link.caption,link.description,link.notes, \
         link.printable,link.printed,link.pre_supported,link.up_axis,link.support_hint, \
         link.orientation_x,link.orientation_y,link.orientation_z,source.lifecycle_state, \
         source.lifecycle_revision FROM volund.models model \
         JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.library_roots root ON root.id=source.library_root_id \
         JOIN volund.content_objects content ON content.id=source.content_object_id \
         WHERE model.public_id::text=$1 AND model.active AND source.lifecycle_state='available' ORDER BY link.is_primary DESC,link.ordinal,source.relative_path \
         LIMIT $2 OFFSET $3",
    )
    .bind(model_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list model file page: {error}"))?;
    Ok(Some(Page {
        items: rows.iter().map(model_file_from_row).collect(),
        limit,
        offset,
        total,
    }))
}

fn model_file_from_row(row: &sqlx::postgres::PgRow) -> ModelFileSummary {
    ModelFileSummary {
        id: row.get(0),
        path: row.get(1),
        root_key: row.get(2),
        root_name: row.get(3),
        role: row.get(4),
        primary: row.get(5),
        format: row.get(6),
        byte_size: row.get(7),
        modified_at_unix_ms: row.get(8),
        missing: row.get(9),
        revision: row.get(10),
        caption: row.get(11),
        description: row.get(12),
        notes: row.get(13),
        printable: row.get(14),
        printed: row.get(15),
        pre_supported: row.get(16),
        up_axis: row.get(17),
        support_hint: row.get(18),
        orientation: [row.get(19), row.get(20), row.get(21)],
        lifecycle_state: row.get(22),
        lifecycle_revision: row.get(23),
    }
}

/// List direct logical children of a project or assembly.
///
/// # Errors
/// Returns a database diagnostic when the relationship graph cannot be loaded.
pub async fn list_components(
    pool: &PgPool,
    model_id: &str,
) -> Result<Option<Vec<ModelComponentSummary>>, String> {
    let rows = sqlx::query(
        "SELECT child.public_id::text,child.name,child.kind,count(files.source_file_id)::bigint \
         FROM volund.models parent JOIN volund.model_components component \
         ON component.parent_model_id=parent.id JOIN volund.models child \
         ON child.id=component.child_model_id LEFT JOIN volund.model_source_files files \
         ON files.model_id=child.id WHERE parent.public_id::text=$1 \
         GROUP BY component.ordinal,child.id ORDER BY component.ordinal,child.name",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list model components: {error}"))?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1)")
            .bind(model_id)
            .fetch_one(pool)
            .await
            .map_err(|error| format!("cannot resolve model: {error}"))?;
    Ok(exists.then(|| {
        rows.iter()
            .map(|row| ModelComponentSummary {
                id: row.get(0),
                name: row.get(1),
                kind: row.get(2),
                file_count: row.get(3),
            })
            .collect()
    }))
}

/// Attach a logical child while refusing self-links and cycles.
///
/// # Errors
/// Returns lookup, conflict, or persistence errors without a partial link.
pub async fn add_component(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    child_model_id: &str,
    expected_revision: i64,
) -> Result<(), ModelError> {
    if model_id == child_model_id {
        return Err(ModelError::Conflict(
            "a model cannot contain itself".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(ModelError::database)?;
    let rows = lock_models(&mut tx, model_id, child_model_id).await?;
    let parent = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == model_id)
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    let child = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == child_model_id)
        .ok_or_else(|| ModelError::NotFound("child model not found".to_owned()))?;
    ensure_revision(parent.get(2), expected_revision)?;
    let parent_id: i64 = parent.get(0);
    let child_id: i64 = child.get(0);
    let cyclic: bool = sqlx::query_scalar(
        "WITH RECURSIVE descendants(id) AS (SELECT $2::bigint UNION \
         SELECT link.child_model_id FROM volund.model_components link \
         JOIN descendants ON descendants.id=link.parent_model_id) \
         SELECT EXISTS(SELECT 1 FROM descendants WHERE id=$1)",
    )
    .bind(parent_id)
    .bind(child_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ModelError::database)?;
    if cyclic {
        return Err(ModelError::Conflict(
            "model component would create a cycle".to_owned(),
        ));
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM volund.model_components WHERE parent_model_id=$1 AND child_model_id=$2)",
    ).bind(parent_id).bind(child_id).fetch_one(&mut *tx).await.map_err(ModelError::database)?;
    if exists {
        return Err(ModelError::Conflict(
            "model component already exists".to_owned(),
        ));
    }
    sqlx::query(
        "INSERT INTO volund.model_components (parent_model_id,child_model_id,ordinal) \
         VALUES ($1,$2,COALESCE((SELECT max(ordinal)+1 FROM volund.model_components \
         WHERE parent_model_id=$1),0))",
    )
    .bind(parent_id)
    .bind(child_id)
    .execute(&mut *tx)
    .await
    .map_err(ModelError::database)?;
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    ).bind(parent_id).fetch_one(&mut *tx).await.map_err(ModelError::database)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "model.component.add",
        "model",
        model_id,
        json!({"childModelId":child_model_id,"revision":revision}),
    )
    .await
    .map_err(ModelError::database)?;
    tx.commit().await.map_err(ModelError::database)?;
    Ok(())
}

/// Remove a logical child without deleting either model.
///
/// # Errors
/// Returns lookup or persistence errors without deleting either model.
pub async fn remove_component(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    child_model_id: &str,
    expected_revision: i64,
) -> Result<(), ModelError> {
    let mut tx = pool.begin().await.map_err(ModelError::database)?;
    let rows = lock_models(&mut tx, model_id, child_model_id).await?;
    let parent = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == model_id)
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    let child = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == child_model_id)
        .ok_or_else(|| ModelError::NotFound("child model not found".to_owned()))?;
    ensure_revision(parent.get(2), expected_revision)?;
    let removed = sqlx::query(
        "DELETE FROM volund.model_components WHERE parent_model_id=$1 AND child_model_id=$2",
    )
    .bind(parent.get::<i64, _>(0))
    .bind(child.get::<i64, _>(0))
    .execute(&mut *tx)
    .await
    .map_err(ModelError::database)?
    .rows_affected();
    if removed == 0 {
        return Err(ModelError::NotFound("model component not found".to_owned()));
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    ).bind(parent.get::<i64, _>(0)).fetch_one(&mut *tx).await.map_err(ModelError::database)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "model.component.remove",
        "model",
        model_id,
        json!({"childModelId":child_model_id,"revision":revision}),
    )
    .await
    .map_err(ModelError::database)?;
    tx.commit().await.map_err(ModelError::database)?;
    Ok(())
}

async fn lock_models(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    first: &str,
    second: &str,
) -> Result<Vec<sqlx::postgres::PgRow>, ModelError> {
    let rows = sqlx::query(
        "SELECT id,public_id::text,revision FROM volund.models WHERE public_id::text IN ($1,$2) \
         ORDER BY id FOR UPDATE",
    )
    .bind(first)
    .bind(second)
    .fetch_all(&mut **tx)
    .await
    .map_err(ModelError::database)?;
    if rows.len() != 2 {
        return Err(ModelError::NotFound(
            "model or child model not found".to_owned(),
        ));
    }
    Ok(rows)
}

fn ensure_revision(current: i64, expected: i64) -> Result<(), ModelError> {
    if expected < 1 {
        return Err(ModelError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    if current != expected {
        return Err(ModelError::RevisionConflict);
    }
    Ok(())
}

fn model_from_row(row: &sqlx::postgres::PgRow) -> ModelSummary {
    let thumbnail_kind: String = row.get(18);
    let source_id: Option<String> = row.get(19);
    let source_missing: Option<bool> = row.get(20);
    let artifact_id: Option<String> = row.get(21);
    let artifact_ready: Option<bool> = row.get(22);
    let generated_id: Option<String> = row.get(24);
    let (candidate_id, url, status) = match thumbnail_kind.as_str() {
        "source-file" if source_missing == Some(false) => {
            let url = source_id
                .as_ref()
                .map(|id| format!("/api/v1/files/{id}/content"));
            (source_id, url, "ready")
        }
        "source-file" => (source_id, None, "fallback"),
        "derived-artifact" if artifact_ready == Some(true) => {
            let url = artifact_id
                .as_ref()
                .map(|id| format!("/api/v1/artifacts/{id}/content"));
            (artifact_id, url, "ready")
        }
        "derived-artifact" => (artifact_id, None, "fallback"),
        _ if generated_id.is_some() => {
            let url = generated_id
                .as_ref()
                .map(|id| format!("/api/v1/artifacts/{id}/content"));
            (None, url, "generated")
        }
        _ => (None, None, "default"),
    };
    ModelSummary {
        id: row.get(0),
        slug: row.get(1),
        name: row.get(2),
        description: row.get(3),
        kind: row.get(4),
        license_kind: row.get(5),
        license_value: row.get(6),
        author_name: row.get(7),
        primary_file_id: row.get(8),
        file_count: row.get(9),
        formats: row.get(10),
        updated_at_unix_ms: row.get(11),
        tags: row.get(12),
        tag_ids: row.get(23),
        collections: row.get(13),
        viewer_rotation: [row.get(14), row.get(15), row.get(16)],
        revision: row.get(17),
        thumbnail: ThumbnailState {
            kind: thumbnail_kind,
            candidate_id,
            url,
            status: status.to_owned(),
        },
    }
}
