use sqlx::{PgPool, Row};

use crate::api_models::{ConfigureImportRequest, ImportConfigurationSummary};
use crate::import_planner::{ImportPlanError, slugify};
use crate::session::AuthenticatedSession;

/// Validate and atomically persist user-reviewed import metadata.
///
/// # Errors
/// Returns an error for invalid metadata, an unknown draft, or a database failure.
#[allow(clippy::too_many_lines)]
pub async fn configure_import(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
    request: ConfigureImportRequest,
) -> Result<ImportConfigurationSummary, ImportPlanError> {
    let model_name = required_text(&request.model_name, "modelName", 160)?;
    let description = limited_text(&request.description, "description", 4_000)?;
    let author_name = optional_text(request.author_name.as_deref(), "authorName", 160)?;
    let kind = validate_kind(&request.kind)?;
    let tags = normalize_tags(request.tags)?;
    let collection_ids = normalize_collection_ids(request.collection_ids)?;
    let (license_kind, license_value) = validate_license(
        request.license_kind.as_deref().unwrap_or("not-specified"),
        request.license_value.as_deref(),
    )?;
    let slug = slugify(model_name).map_err(ImportPlanError::BadRequest)?;
    let mut transaction = pool.begin().await.map_err(|error| {
        ImportPlanError::Database(format!("cannot begin import configuration: {error}"))
    })?;
    let library_root_id = required_text(&request.library_root_id, "libraryRootId", 36)?;
    let resolved_root_id = resolve_root(&mut transaction, library_root_id).await?;
    let resolved_collections = resolve_collections(&mut transaction, &collection_ids).await?;
    let target_action = validate_target_action(request.target_action.as_deref())?;
    let (target_model_id, target_model_public_id, target_model_revision) = resolve_target_model(
        &mut transaction,
        target_action,
        request.target_model_id.as_deref(),
        request.expected_model_revision,
    )
    .await?;
    let row = sqlx::query(
        "UPDATE volund.import_drafts SET suggested_model_name=$2,suggested_slug=$3, \
         model_kind=$4,description=$5,author_name=$6,tags=$7,target_library_root_id=$8, \
         target_model_id=$9,target_action=$10,target_model_revision=$11,license_kind=$12, \
         license_value=$13,planned_base_directory=NULL,reviewed_at=NULL,configured_at=now(), \
         updated_at=now() WHERE public_id::text=$1 AND owner_user_id=$14 AND status IN \
         ('draft','failed','uploading','uploaded','review_ready','reviewed') \
         RETURNING id,public_id::text",
    )
    .bind(draft_id)
    .bind(model_name)
    .bind(&slug)
    .bind(kind)
    .bind(description)
    .bind(author_name.as_deref())
    .bind(&tags)
    .bind(resolved_root_id)
    .bind(target_model_id)
    .bind(target_action)
    .bind(target_model_revision)
    .bind(&license_kind)
    .bind(license_value.as_deref())
    .bind(actor.database_user_id())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("configure import draft"))?
    .ok_or_else(|| ImportPlanError::NotFound(format!("unknown active import draft: {draft_id}")))?;
    let import_draft_id: i64 = row.get(0);
    if target_action == "extend" && request.primary_item_id.is_some() {
        return Err(ImportPlanError::BadRequest(
            "model extension cannot replace the primary source".to_owned(),
        ));
    }
    let (primary_item_id, thumbnail_item_id) = resolve_item_selections(
        &mut transaction,
        import_draft_id,
        request.primary_item_id.as_deref(),
        request.thumbnail_item_id.as_deref(),
        target_action != "extend",
    )
    .await?;
    sqlx::query(
        "UPDATE volund.import_drafts SET primary_item_id=$2,thumbnail_item_id=$3 WHERE id=$1",
    )
    .bind(import_draft_id)
    .bind(primary_item_id)
    .bind(thumbnail_item_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("save import file choices"))?;
    reset_review_items(&mut transaction, import_draft_id).await?;
    sqlx::query("DELETE FROM volund.import_draft_collections WHERE import_draft_id=$1")
        .bind(import_draft_id)
        .execute(&mut *transaction)
        .await
        .map_err(database_error("reset import collections"))?;
    for (ordinal, collection_id) in resolved_collections.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO volund.import_draft_collections \
             (import_draft_id,collection_id,ordinal) VALUES ($1,$2,$3)",
        )
        .bind(import_draft_id)
        .bind(collection_id)
        .bind(
            i32::try_from(ordinal).map_err(|_| {
                ImportPlanError::BadRequest("too many import collections".to_owned())
            })?,
        )
        .execute(&mut *transaction)
        .await
        .map_err(database_error("configure import collection"))?;
    }
    transaction.commit().await.map_err(|error| {
        ImportPlanError::Database(format!("cannot commit import configuration: {error}"))
    })?;
    Ok(ImportConfigurationSummary {
        id: draft_id.to_owned(),
        model_name: model_name.to_owned(),
        slug,
        kind: kind.to_owned(),
        library_root_id: library_root_id.to_owned(),
        description: description.to_owned(),
        author_name,
        tags,
        collection_ids,
        ready_for_upload: true,
        target_action: target_action.to_owned(),
        target_model_id: target_model_public_id,
        expected_model_revision: target_model_revision,
        license_kind,
        license_value,
        primary_item_id: request.primary_item_id,
        thumbnail_item_id: request.thumbnail_item_id,
    })
}

fn validate_license(
    kind: &str,
    value: Option<&str>,
) -> Result<(String, Option<String>), ImportPlanError> {
    match kind {
        "not-specified" if value.is_none_or(|value| value.trim().is_empty()) => {
            Ok((kind.to_owned(), None))
        }
        "spdx" | "custom" => Ok((
            kind.to_owned(),
            Some(required_text(value.unwrap_or_default(), "licenseValue", 160)?.to_owned()),
        )),
        _ => Err(ImportPlanError::BadRequest(
            "invalid license selection".to_owned(),
        )),
    }
}

fn validate_target_action(action: Option<&str>) -> Result<&str, ImportPlanError> {
    match action.unwrap_or("create") {
        action @ ("create" | "update" | "extend") => Ok(action),
        _ => Err(ImportPlanError::BadRequest(
            "targetAction must be create, update, or extend".to_owned(),
        )),
    }
}

async fn resolve_item_selections(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    draft_id: i64,
    primary: Option<&str>,
    thumbnail: Option<&str>,
    suggest_primary: bool,
) -> Result<(Option<i64>, Option<i64>), ImportPlanError> {
    let primary_id = if let Some(value) = primary {
        Some(
            resolve_item_kind(
                transaction,
                draft_id,
                value,
                &["cad", "mesh"],
                "primary source",
                false,
            )
            .await?,
        )
    } else if suggest_primary {
        sqlx::query_scalar("SELECT id FROM volund.import_draft_items WHERE import_draft_id=$1 AND is_primary_candidate ORDER BY id LIMIT 1")
            .bind(draft_id).fetch_optional(&mut **transaction).await
            .map_err(database_error("suggest primary source"))?
    } else {
        None
    };
    let thumbnail_id = if let Some(value) = thumbnail {
        Some(resolve_item_kind(transaction, draft_id, value, &["image"], "thumbnail", true).await?)
    } else {
        None
    };
    Ok((primary_id, thumbnail_id))
}

async fn resolve_item_kind(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    draft_id: i64,
    public_id: &str,
    categories: &[&str],
    label: &str,
    raster_only: bool,
) -> Result<i64, ImportPlanError> {
    sqlx::query_scalar("SELECT id FROM volund.import_draft_items WHERE import_draft_id=$1 AND public_id::text=$2 AND category=ANY($3) AND (NOT $4 OR lower(original_path)!~'\\.svg$')")
        .bind(draft_id).bind(public_id).bind(categories).bind(raster_only)
        .fetch_optional(&mut **transaction).await
        .map_err(|error| ImportPlanError::Database(format!("cannot resolve {label}: {error}")))?
        .ok_or_else(|| ImportPlanError::BadRequest(format!("{label} must belong to the draft and use a supported type")))
}

async fn resolve_target_model(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    action: &str,
    public_id: Option<&str>,
    expected_revision: Option<i64>,
) -> Result<(Option<i64>, Option<String>, Option<i64>), ImportPlanError> {
    if action == "create" {
        if public_id.is_some() || expected_revision.is_some() {
            return Err(ImportPlanError::BadRequest(
                "create target must not include a model or revision".to_owned(),
            ));
        }
        return Ok((None, None, None));
    }
    let public_id = public_id.ok_or_else(|| {
        ImportPlanError::BadRequest("existing-model target requires targetModelId".to_owned())
    })?;
    let revision = expected_revision
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            ImportPlanError::BadRequest(
                "existing-model target requires expectedModelRevision".to_owned(),
            )
        })?;
    let row = sqlx::query(
        "SELECT id,public_id::text,revision FROM volund.models WHERE public_id::text=$1",
    )
    .bind(public_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("resolve target model"))?
    .ok_or_else(|| ImportPlanError::NotFound("unknown target model".to_owned()))?;
    if row.get::<i64, _>(2) != revision {
        return Err(ImportPlanError::BadRequest(
            "target model revision changed; reload the model".to_owned(),
        ));
    }
    Ok((Some(row.get(0)), Some(row.get(1)), Some(revision)))
}

async fn resolve_root(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    public_id: &str,
) -> Result<i64, ImportPlanError> {
    sqlx::query_scalar("SELECT id FROM volund.library_roots WHERE public_id::text=$1")
        .bind(public_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("resolve import library"))?
        .ok_or_else(|| ImportPlanError::NotFound(format!("unknown library root: {public_id}")))
}

async fn resolve_collections(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    public_ids: &[String],
) -> Result<Vec<i64>, ImportPlanError> {
    let mut resolved = Vec::with_capacity(public_ids.len());
    for public_id in public_ids {
        let id = sqlx::query_scalar(
            "SELECT id FROM volund.collections WHERE public_id::text=$1 AND active",
        )
        .bind(public_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("resolve import collection"))?
        .ok_or_else(|| ImportPlanError::NotFound(format!("unknown collection: {public_id}")))?;
        resolved.push(id);
    }
    Ok(resolved)
}

async fn reset_review_items(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    draft_id: i64,
) -> Result<(), ImportPlanError> {
    sqlx::query("UPDATE volund.import_draft_items SET planned_action=NULL,planned_relative_path=NULL,matched_source_file_id=NULL WHERE import_draft_id=$1")
        .bind(draft_id).execute(&mut **transaction).await
        .map_err(database_error("reset import review items"))?;
    Ok(())
}

pub(crate) fn required_text<'a>(
    value: &'a str,
    field: &str,
    max: usize,
) -> Result<&'a str, ImportPlanError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        Err(ImportPlanError::BadRequest(format!(
            "{field} must contain between 1 and {max} characters"
        )))
    } else {
        Ok(value)
    }
}

fn limited_text<'a>(value: &'a str, field: &str, max: usize) -> Result<&'a str, ImportPlanError> {
    let value = value.trim();
    if value.chars().count() > max {
        Err(ImportPlanError::BadRequest(format!(
            "{field} must not exceed {max} characters"
        )))
    } else {
        Ok(value)
    }
}

pub(crate) fn optional_text(
    value: Option<&str>,
    field: &str,
    max: usize,
) -> Result<Option<String>, ImportPlanError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| limited_text(value, field, max).map(ToOwned::to_owned))
        .transpose()
}

pub(crate) fn validate_kind(kind: &str) -> Result<&str, ImportPlanError> {
    match kind {
        "part" | "assembly" | "project" => Ok(kind),
        _ => Err(ImportPlanError::BadRequest(
            "kind must be part, assembly, or project".to_owned(),
        )),
    }
}

pub(crate) fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>, ImportPlanError> {
    if tags.len() > 30 {
        return Err(ImportPlanError::BadRequest(
            "an import draft supports at most 30 tags".to_owned(),
        ));
    }
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = required_text(&tag, "tag", 50)?.to_owned();
        if !normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&tag))
        {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}

pub(crate) fn normalize_collection_ids(ids: Vec<String>) -> Result<Vec<String>, ImportPlanError> {
    if ids.len() > 20 {
        return Err(ImportPlanError::BadRequest(
            "an import draft supports at most 20 collections".to_owned(),
        ));
    }
    let mut normalized = Vec::new();
    for id in ids {
        let id = required_text(&id, "collectionId", 36)?.to_owned();
        if !normalized.contains(&id) {
            normalized.push(id);
        }
    }
    Ok(normalized)
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> ImportPlanError {
    move |error| ImportPlanError::Database(format!("cannot {context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::validate_target_action;

    #[test]
    fn target_action_distinguishes_metadata_update_from_safe_extension() {
        assert_eq!(validate_target_action(None).unwrap(), "create");
        assert_eq!(validate_target_action(Some("update")).unwrap(), "update");
        assert_eq!(validate_target_action(Some("extend")).unwrap(), "extend");
        assert!(validate_target_action(Some("replace")).is_err());
    }
}
