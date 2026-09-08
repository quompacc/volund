use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api_models::ModelSummary;
use crate::model_maintenance::ModelError;
use crate::session::AuthenticatedSession;

const RASTER_MEDIA: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailCandidate {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub url: String,
    pub media_type: String,
    pub byte_size: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailUpdate {
    pub expected_revision: i64,
    pub kind: String,
    pub candidate_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThumbnailRegenerate {
    pub expected_revision: i64,
    pub profile: String,
}

/// List bounded safe raster candidates that belong to a model.
///
/// # Errors
/// Returns lookup or database errors without exposing filesystem paths.
pub async fn candidates(
    pool: &PgPool,
    model_id: &str,
) -> Result<Vec<ThumbnailCandidate>, ModelError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1)")
            .bind(model_id)
            .fetch_one(pool)
            .await
            .map_err(ModelError::database)?;
    if !exists {
        return Err(ModelError::NotFound("model not found".to_owned()));
    }
    let sources = sqlx::query(
        "SELECT source.public_id::text,source.relative_path,content.byte_size, \
         lower(substring(source.relative_path FROM '\\.([^.]*)$')) FROM volund.models model \
         JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.content_objects content ON content.id=source.content_object_id \
         WHERE model.public_id::text=$1 AND source.missing_at IS NULL \
         AND lower(substring(source.relative_path FROM '\\.([^.]*)$'))=ANY($2) \
         ORDER BY link.ordinal,source.relative_path,source.id LIMIT 100",
    )
    .bind(model_id)
    .bind(["png", "jpg", "jpeg", "gif", "webp"])
    .fetch_all(pool)
    .await
    .map_err(ModelError::database)?;
    let artifacts = sqlx::query(
        "SELECT artifact.public_id::text,artifact.artifact_kind,artifact.media_type,artifact.byte_size \
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.conversion_runs run ON run.content_object_id=source.content_object_id \
         JOIN volund.derived_artifacts artifact ON artifact.conversion_run_id=run.id \
         WHERE model.public_id::text=$1 AND run.status='ready' AND artifact.media_type=ANY($2) \
         ORDER BY run.finished_at DESC NULLS LAST,artifact.id DESC LIMIT 100",
    )
    .bind(model_id).bind(RASTER_MEDIA).fetch_all(pool).await.map_err(ModelError::database)?;
    let mut result = Vec::with_capacity(sources.len() + artifacts.len());
    result.extend(sources.iter().map(|row| {
        let id: String = row.get(0);
        let path: String = row.get(1);
        let extension: String = row.get(3);
        ThumbnailCandidate {
            id: id.clone(),
            kind: "source-file".to_owned(),
            label: path,
            url: format!("/api/v1/files/{id}/content"),
            media_type: source_media(&extension).to_owned(),
            byte_size: row.get(2),
        }
    }));
    result.extend(artifacts.iter().map(|row| {
        let id: String = row.get(0);
        ThumbnailCandidate {
            id: id.clone(),
            kind: "derived-artifact".to_owned(),
            label: row.get(1),
            url: format!("/api/v1/artifacts/{id}/content"),
            media_type: row.get(2),
            byte_size: row.get(3),
        }
    }));
    let mut seen = std::collections::HashSet::new();
    result.retain(|candidate| seen.insert((candidate.kind.clone(), candidate.id.clone())));
    Ok(result)
}

/// Select a safe thumbnail candidate or the deliberate default with revision control.
///
/// # Errors
/// Returns validation, ownership, readiness, revision, or persistence errors atomically.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    request: &ThumbnailUpdate,
) -> Result<ModelSummary, ModelError> {
    if request.expected_revision < 1 {
        return Err(ModelError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(ModelError::database)?;
    let model =
        sqlx::query("SELECT id,revision FROM volund.models WHERE public_id::text=$1 FOR UPDATE")
            .bind(model_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ModelError::database)?
            .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    if model.get::<i64, _>(1) != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    let model_internal: i64 = model.get(0);
    let (source_id, artifact_id) = match (request.kind.as_str(), request.candidate_id.as_deref()) {
        ("default", None) => (None, None),
        ("source-file", Some(id)) => (
            Some(lock_source_candidate(&mut tx, model_internal, id).await?),
            None,
        ),
        ("derived-artifact", Some(id)) => (
            None,
            Some(lock_artifact_candidate(&mut tx, model_internal, id).await?),
        ),
        _ => {
            return Err(ModelError::BadRequest(
                "thumbnail kind and candidate do not form a valid selection".to_owned(),
            ));
        }
    };
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET thumbnail_kind=$2,thumbnail_source_file_id=$3, \
         thumbnail_artifact_id=$4,revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    ).bind(model_internal).bind(&request.kind).bind(source_id).bind(artifact_id)
        .fetch_one(&mut *tx).await.map_err(ModelError::database)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "model.thumbnail.update",
        "model",
        model_id,
        json!({"kind":request.kind,"candidateId":request.candidate_id,"revision":revision}),
    )
    .await
    .map_err(ModelError::database)?;
    tx.commit().await.map_err(ModelError::database)?;
    crate::model_catalog::get_model(pool, model_id)
        .await
        .map_err(ModelError::Database)?
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))
}

/// Enqueue a fresh preview for the model's preferred available CAD or mesh source.
///
/// # Errors
/// Returns validation, stale-revision, missing-source, profile, or database errors.
pub async fn regenerate(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    request: &ThumbnailRegenerate,
) -> Result<crate::preview_pipeline::PreviewRequest, ModelError> {
    if request.expected_revision < 1 || !matches!(request.profile.as_str(), "web" | "fine") {
        return Err(ModelError::BadRequest(
            "thumbnail regeneration requires a current revision and supported profile".to_owned(),
        ));
    }
    let row = sqlx::query(
        "SELECT model.revision,source.public_id::text FROM volund.models model
         JOIN volund.model_source_files link ON link.model_id=model.id
         JOIN volund.source_files source ON source.id=link.source_file_id
         JOIN volund.content_objects content ON content.id=source.content_object_id
         WHERE model.public_id::text=$1 AND model.active AND source.lifecycle_state='available'
         AND source.missing_at IS NULL AND content.detected_format=ANY($2)
         ORDER BY link.is_primary DESC,link.ordinal,source.id LIMIT 1",
    )
    .bind(model_id)
    .bind([
        "step", "iges", "brep", "stl", "3mf", "obj", "ply", "gltf", "glb",
    ])
    .fetch_optional(pool)
    .await
    .map_err(ModelError::database)?
    .ok_or_else(|| ModelError::BadRequest("model has no available renderable source".to_owned()))?;
    if row.get::<i64, _>(0) != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    let source_id: String = row.get(1);
    let preview = crate::preview_pipeline::regenerate(pool, &source_id, &request.profile)
        .await
        .map_err(ModelError::Database)?;
    let mut tx = pool.begin().await.map_err(ModelError::database)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "model.thumbnail.regenerate",
        "model",
        model_id,
        json!({"previewId":&preview.id,"profile":&request.profile,"revision":request.expected_revision}),
    )
    .await
    .map_err(ModelError::database)?;
    tx.commit().await.map_err(ModelError::database)?;
    Ok(preview)
}

async fn lock_source_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    model_id: i64,
    candidate_id: &str,
) -> Result<i64, ModelError> {
    let row = sqlx::query(
        "SELECT source.id,lower(substring(source.relative_path FROM '\\.([^.]*)$')) \
         FROM volund.model_source_files link JOIN volund.source_files source ON source.id=link.source_file_id \
         WHERE link.model_id=$1 AND source.public_id::text=$2 AND source.missing_at IS NULL FOR UPDATE OF source",
    ).bind(model_id).bind(candidate_id).fetch_optional(&mut **tx).await.map_err(ModelError::database)?
        .ok_or_else(|| ModelError::BadRequest("thumbnail source must be an available associated file".to_owned()))?;
    let extension: String = row.get(1);
    if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        return Err(ModelError::BadRequest(
            "thumbnail source must be a supported raster image".to_owned(),
        ));
    }
    Ok(row.get(0))
}

async fn lock_artifact_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    model_id: i64,
    candidate_id: &str,
) -> Result<i64, ModelError> {
    let row = sqlx::query(
        "SELECT artifact.id,artifact.media_type FROM volund.model_source_files link \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.conversion_runs run ON run.content_object_id=source.content_object_id \
         JOIN volund.derived_artifacts artifact ON artifact.conversion_run_id=run.id \
         WHERE link.model_id=$1 AND artifact.public_id::text=$2 AND run.status='ready' FOR UPDATE OF artifact",
    ).bind(model_id).bind(candidate_id).fetch_optional(&mut **tx).await.map_err(ModelError::database)?
        .ok_or_else(|| ModelError::BadRequest("thumbnail artifact must be ready and belong to associated content".to_owned()))?;
    let media_type: String = row.get(1);
    if !RASTER_MEDIA.contains(&media_type.as_str()) {
        return Err(ModelError::BadRequest(
            "thumbnail artifact must be a supported raster image".to_owned(),
        ));
    }
    Ok(row.get(0))
}

fn source_media(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_raster_media_types_are_thumbnail_safe() {
        assert!(RASTER_MEDIA.contains(&"image/png"));
        assert!(!RASTER_MEDIA.contains(&"image/svg+xml"));
        assert_eq!(source_media("jpeg"), "image/jpeg");
        assert_eq!(source_media("svg"), "application/octet-stream");
    }
}
