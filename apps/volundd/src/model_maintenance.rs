use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::api_models::{
    CreateModelRequest, ModelSummary, SetModelPrimaryRequest, UpdateModelRequest,
};
use crate::session::AuthenticatedSession;

const SPDX_LICENSES: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "CERN-OHL-P-2.0",
    "CERN-OHL-S-2.0",
    "CERN-OHL-W-2.0",
    "GPL-3.0-only",
    "MIT",
    "Unlicense",
];

#[derive(Debug)]
pub enum ModelError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    RevisionConflict,
    Database(String),
}

impl ModelError {
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn database(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            Self::Conflict("catalog identity already exists".to_owned())
        } else {
            Self::Database(format!("cannot persist model: {error}"))
        }
    }
}

/// Create a model with one stable primary source and actor evidence.
///
/// # Errors
/// Returns validation, lookup, conflict, or database errors atomically.
pub async fn create_model(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    request: CreateModelRequest,
) -> Result<ModelSummary, ModelError> {
    validate_create(&request)?;
    let mut transaction = pool.begin().await.map_err(ModelError::database)?;
    let source = sqlx::query(
        "SELECT source.id,content.detected_format,source.missing_at IS NOT NULL \
         FROM volund.source_files source JOIN volund.content_objects content \
         ON content.id=source.content_object_id WHERE source.public_id::text=$1 FOR UPDATE OF source",
    )
    .bind(&request.primary_file_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ModelError::database)?
    .ok_or_else(|| ModelError::NotFound("unknown primary source file".to_owned()))?;
    if source.get::<bool, _>(2) {
        return Err(ModelError::NotFound(
            "primary source file is missing".to_owned(),
        ));
    }
    let format: Option<String> = source.get(1);
    let role = primary_role(format.as_deref()).ok_or_else(|| {
        ModelError::BadRequest("primary source file must be CAD or mesh".to_owned())
    })?;
    let row = sqlx::query(
        "INSERT INTO volund.models (slug,name,kind) VALUES ($1,$2,$3) \
         RETURNING id,public_id::text",
    )
    .bind(&request.slug)
    .bind(request.name.trim())
    .bind(&request.kind)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ModelError::database)?;
    let internal_id: i64 = row.get(0);
    let public_id: String = row.get(1);
    sqlx::query(
        "INSERT INTO volund.model_source_files \
         (model_id,source_file_id,role,is_primary) VALUES ($1,$2,$3,true)",
    )
    .bind(internal_id)
    .bind(source.get::<i64, _>(0))
    .bind(role)
    .execute(&mut *transaction)
    .await
    .map_err(ModelError::database)?;
    audit(
        &mut transaction,
        actor,
        "model.create",
        &public_id,
        json!({"fields":["name","kind","primaryFileId"],"revision":1}),
    )
    .await?;
    transaction.commit().await.map_err(ModelError::database)?;
    crate::model_catalog::get_model(pool, &public_id)
        .await
        .map_err(ModelError::Database)?
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))
}

/// Atomically replace editable metadata and the deliberate primary source.
///
/// # Errors
/// Returns validation, lookup, revision-conflict, or database errors atomically.
pub async fn update_model(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    request: UpdateModelRequest,
) -> Result<ModelSummary, ModelError> {
    let license = validate_update(&request)?;
    let mut transaction = pool.begin().await.map_err(ModelError::database)?;
    let row =
        sqlx::query("SELECT id,revision FROM volund.models WHERE public_id::text=$1 FOR UPDATE")
            .bind(model_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(ModelError::database)?
            .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    let internal_id: i64 = row.get(0);
    if row.get::<i64, _>(1) != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    let author_id = persist_author(&mut transaction, actor, request.author_name.as_deref()).await?;
    set_primary(
        &mut transaction,
        internal_id,
        request.primary_file_id.as_deref(),
    )
    .await?;
    let new_revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET name=$2,description=$3,kind=$4,author_id=$5, \
         license_kind=$6,license_value=$7,viewer_rotation_x=$8,viewer_rotation_y=$9, \
         viewer_rotation_z=$10,updated_at=now(),revision=revision+1 WHERE id=$1 RETURNING revision",
    )
    .bind(internal_id)
    .bind(request.name.trim())
    .bind(request.description.trim())
    .bind(&request.kind)
    .bind(author_id)
    .bind(&request.license_kind)
    .bind(license)
    .bind(request.viewer_rotation[0])
    .bind(request.viewer_rotation[1])
    .bind(request.viewer_rotation[2])
    .fetch_one(&mut *transaction)
    .await
    .map_err(ModelError::database)?;
    crate::tag_catalog::replace_model_tags(
        &mut transaction,
        actor,
        internal_id,
        &request.tags,
        request.tag_ids.as_deref(),
    )
    .await
    .map_err(|error| match error {
        crate::tag_catalog::TagError::BadRequest(message) => ModelError::BadRequest(message),
        crate::tag_catalog::TagError::Conflict(message) => ModelError::Conflict(message),
        crate::tag_catalog::TagError::Database(message) => ModelError::Database(message),
        crate::tag_catalog::TagError::NotFound => ModelError::NotFound("tag not found".to_owned()),
        crate::tag_catalog::TagError::RevisionConflict => ModelError::RevisionConflict,
    })?;
    replace_collections(&mut transaction, internal_id, &request.collection_ids).await?;
    audit(
        &mut transaction,
        actor,
        "model.update",
        model_id,
        json!({
            "fields":["name","description","kind","license","author","tags",
                "collections","primaryFileId","viewerRotation"], "revision":new_revision
        }),
    )
    .await?;
    transaction.commit().await.map_err(ModelError::database)?;
    crate::model_catalog::get_model(pool, model_id)
        .await
        .map_err(ModelError::Database)?
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))
}

/// Select one already-linked CAD or mesh source without replacing model metadata.
///
/// # Errors
/// Returns validation, lookup, revision-conflict, or database errors atomically.
pub async fn update_primary(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    request: SetModelPrimaryRequest,
) -> Result<ModelSummary, ModelError> {
    if request.expected_revision < 1 || request.primary_file_id.is_empty() {
        return Err(ModelError::BadRequest(
            "invalid primary source selection".to_owned(),
        ));
    }
    let mut transaction = pool.begin().await.map_err(ModelError::database)?;
    let row = sqlx::query(
        "SELECT id,revision FROM volund.models WHERE public_id::text=$1 AND active FOR UPDATE",
    )
    .bind(model_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ModelError::database)?
    .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))?;
    if row.get::<i64, _>(1) != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    set_primary(&mut transaction, row.get(0), Some(&request.primary_file_id)).await?;
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision",
    )
    .bind(row.get::<i64, _>(0))
    .fetch_one(&mut *transaction)
    .await
    .map_err(ModelError::database)?;
    audit(&mut transaction, actor, "model.primary.update", model_id,
        json!({"fields":["primaryFileId"],"primaryFileId":request.primary_file_id,"revision":revision})).await?;
    transaction.commit().await.map_err(ModelError::database)?;
    crate::model_catalog::get_model(pool, model_id)
        .await
        .map_err(ModelError::Database)?
        .ok_or_else(|| ModelError::NotFound("model not found".to_owned()))
}

fn validate_create(request: &CreateModelRequest) -> Result<(), ModelError> {
    if request.name.trim().is_empty() || request.name.trim().chars().count() > 160 {
        return Err(ModelError::BadRequest(
            "model name must contain 1 to 160 characters".to_owned(),
        ));
    }
    validate_kind(&request.kind)?;
    let valid_slug = !request.slug.is_empty()
        && request.slug.chars().count() <= 160
        && request.slug.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        });
    if !valid_slug {
        return Err(ModelError::BadRequest(
            "model slug must contain 1 to 160 lowercase letters, digits, and single hyphens"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_update(request: &UpdateModelRequest) -> Result<Option<String>, ModelError> {
    if request.expected_revision < 1 {
        return Err(ModelError::BadRequest(
            "expected revision must be positive".to_owned(),
        ));
    }
    if request.name.trim().is_empty() || request.name.trim().chars().count() > 160 {
        return Err(ModelError::BadRequest(
            "model name must contain 1 to 160 characters".to_owned(),
        ));
    }
    if request.description.trim().chars().count() > 4_000 {
        return Err(ModelError::BadRequest(
            "model description exceeds 4000 characters".to_owned(),
        ));
    }
    validate_kind(&request.kind)?;
    if request.tags.len() > 30
        || request.tag_ids.as_ref().is_some_and(|ids| ids.len() > 30)
        || request.collection_ids.len() > 20
    {
        return Err(ModelError::BadRequest(
            "too many tags or collections".to_owned(),
        ));
    }
    if request
        .viewer_rotation
        .iter()
        .any(|value| !value.is_finite() || !(-360.0..=360.0).contains(value))
    {
        return Err(ModelError::BadRequest(
            "viewer rotation must contain finite degrees between -360 and 360".to_owned(),
        ));
    }
    validate_license(&request.license_kind, request.license_value.as_deref())
}

fn validate_kind(kind: &str) -> Result<(), ModelError> {
    if matches!(kind, "part" | "assembly" | "project") {
        Ok(())
    } else {
        Err(ModelError::BadRequest(
            "model kind must be part, assembly, or project".to_owned(),
        ))
    }
}

fn validate_license(kind: &str, value: Option<&str>) -> Result<Option<String>, ModelError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    match (kind, value) {
        ("not-specified", None) => Ok(None),
        ("spdx", Some(value)) if SPDX_LICENSES.contains(&value) => Ok(Some(value.to_owned())),
        ("custom", Some(value)) if value.chars().count() <= 160 => Ok(Some(value.to_owned())),
        _ => Err(ModelError::BadRequest(
            "license must be not specified, a recognized SPDX identifier, or bounded custom text"
                .to_owned(),
        )),
    }
}

fn primary_role(format: Option<&str>) -> Option<&'static str> {
    match format {
        Some("stl" | "3mf" | "obj" | "ply") => Some("printable-mesh"),
        Some("step" | "iges" | "brep") => Some("master-cad"),
        _ => None,
    }
}

async fn set_primary(
    transaction: &mut Transaction<'_, Postgres>,
    model_id: i64,
    source_public_id: Option<&str>,
) -> Result<(), ModelError> {
    let selected = if let Some(public_id) = source_public_id {
        let row = sqlx::query(
            "SELECT link.source_file_id,link.role,content.detected_format \
             FROM volund.model_source_files link JOIN volund.source_files source \
             ON source.id=link.source_file_id JOIN volund.content_objects content \
             ON content.id=source.content_object_id WHERE link.model_id=$1 \
             AND source.public_id::text=$2 FOR UPDATE OF link",
        )
        .bind(model_id)
        .bind(public_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ModelError::database)?
        .ok_or_else(|| {
            ModelError::BadRequest("primary file must belong to the model".to_owned())
        })?;
        let role: String = row.get(1);
        let format: Option<String> = row.get(2);
        if !matches!(role.as_str(), "master-cad" | "cad" | "printable-mesh")
            || !format.as_deref().is_some_and(allowed_primary_format)
        {
            return Err(ModelError::BadRequest(
                "primary file must be an associated CAD or mesh source".to_owned(),
            ));
        }
        Some(row.get::<i64, _>(0))
    } else {
        None
    };
    sqlx::query("UPDATE volund.model_source_files SET is_primary=false WHERE model_id=$1")
        .bind(model_id)
        .execute(&mut **transaction)
        .await
        .map_err(ModelError::database)?;
    if let Some(source_id) = selected {
        sqlx::query("UPDATE volund.model_source_files SET is_primary=true WHERE model_id=$1 AND source_file_id=$2")
            .bind(model_id).bind(source_id).execute(&mut **transaction).await
            .map_err(ModelError::database)?;
    }
    Ok(())
}

fn allowed_primary_format(format: &str) -> bool {
    matches!(
        format,
        "step" | "iges" | "brep" | "stl" | "3mf" | "obj" | "ply"
    )
}

async fn persist_author(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    name: Option<&str>,
) -> Result<Option<i64>, ModelError> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    let (name, normalized) = crate::metadata_normalization::normalize_identity(name, 160)
        .map_err(ModelError::BadRequest)?;
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM volund.authors WHERE normalized_name=$1 AND active FOR UPDATE",
    )
    .bind(&normalized)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(ModelError::database)?
    {
        return Ok(Some(id));
    }
    let row = sqlx::query(
        "INSERT INTO volund.authors (name,normalized_name,provenance_source) \
         VALUES ($1,$2,'user') RETURNING id,public_id::text",
    )
    .bind(name)
    .bind(normalized)
    .fetch_one(&mut **transaction)
    .await
    .map_err(ModelError::database)?;
    crate::catalog_audit::record(
        transaction,
        actor,
        "author.create",
        "author",
        &row.get::<String, _>(1),
        json!({"fields":["name"],"context":"model-maintenance","revision":1}),
    )
    .await
    .map_err(ModelError::database)?;
    Ok(Some(row.get(0)))
}

async fn replace_collections(
    transaction: &mut Transaction<'_, Postgres>,
    model_id: i64,
    ids: &[String],
) -> Result<(), ModelError> {
    sqlx::query("DELETE FROM volund.collection_models WHERE model_id=$1")
        .bind(model_id)
        .execute(&mut **transaction)
        .await
        .map_err(ModelError::database)?;
    for (ordinal, public_id) in ids.iter().enumerate() {
        let collection_id: i64 = sqlx::query_scalar(
            "SELECT id FROM volund.collections WHERE public_id::text=$1 AND active",
        )
        .bind(public_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ModelError::database)?
        .ok_or_else(|| ModelError::NotFound("collection not found".to_owned()))?;
        sqlx::query("INSERT INTO volund.collection_models (collection_id,model_id,ordinal) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(collection_id).bind(model_id).bind(i32::try_from(ordinal).unwrap_or(i32::MAX))
            .execute(&mut **transaction).await.map_err(ModelError::database)?;
    }
    Ok(())
}

async fn audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    target_id: &str,
    metadata: serde_json::Value,
) -> Result<(), ModelError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) \
         VALUES ($1,$2::uuid,$3,$4,'success','model',$5::uuid,$6)",
    ).bind(actor.database_user_id()).bind(&actor.user_id).bind(&actor.display_name)
    .bind(action).bind(target_id).bind(metadata).execute(&mut **transaction).await
    .map_err(ModelError::database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update() -> UpdateModelRequest {
        UpdateModelRequest {
            expected_revision: 1,
            name: "Voron Legacy".to_owned(),
            description: "Drucker".to_owned(),
            kind: "project".to_owned(),
            license_kind: "spdx".to_owned(),
            license_value: Some("CERN-OHL-S-2.0".to_owned()),
            author_name: Some("Voron Design".to_owned()),
            tags: vec!["CoreXY".to_owned()],
            tag_ids: None,
            collection_ids: vec!["collection-id".to_owned()],
            primary_file_id: Some("file-id".to_owned()),
            viewer_rotation: [-90.0, 0.0, 0.0],
        }
    }

    #[test]
    fn license_and_model_metadata_are_bounded() {
        assert_eq!(
            validate_update(&update()).unwrap().as_deref(),
            Some("CERN-OHL-S-2.0")
        );
        let invalid = UpdateModelRequest {
            license_value: Some("invented-id".to_owned()),
            ..update()
        };
        assert!(validate_update(&invalid).is_err());
        let invalid = UpdateModelRequest {
            viewer_rotation: [f64::NAN, 0.0, 0.0],
            ..update()
        };
        assert!(validate_update(&invalid).is_err());
    }

    #[test]
    fn primary_formats_follow_cad_and_mesh_semantics() {
        assert!(allowed_primary_format("step"));
        assert!(allowed_primary_format("stl"));
        assert!(!allowed_primary_format("pdf"));
        assert_eq!(primary_role(Some("stl")), Some("printable-mesh"));
        assert_eq!(primary_role(Some("step")), Some("master-cad"));
        assert_eq!(primary_role(Some("pdf")), None);
    }

    #[test]
    fn direct_model_slugs_are_bounded() {
        let valid = CreateModelRequest {
            name: "Unicode Modell Ä".to_owned(),
            slug: "a".repeat(160),
            kind: "part".to_owned(),
            primary_file_id: "file-id".to_owned(),
        };
        assert!(validate_create(&valid).is_ok());
        assert!(
            validate_create(&CreateModelRequest {
                slug: "a".repeat(161),
                ..valid
            })
            .is_err()
        );
    }
}
