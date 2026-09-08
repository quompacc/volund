use std::path::Path;

use sqlx::{Postgres, Row, Transaction};
use volund_core::CadFormat;

use crate::import_commit::{Draft, ImportCommitError, Item};
use crate::metadata_normalization::normalize_identity;
use crate::session::AuthenticatedSession;

pub(crate) async fn persist_import(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    draft: &Draft,
    items: &[Item],
) -> Result<(String, i64, i64, i64), ImportCommitError> {
    let extension = draft.target_action == "extend";
    let author_id = if extension {
        None
    } else {
        persist_author(transaction, actor, draft.author_name.as_deref()).await?
    };
    let (model_id, model_public_id, new_model) =
        persist_model(transaction, draft, author_id).await?;
    let create_count = action_count(items, "create")?;
    let reuse_count = action_count(items, "reuse")?;
    let relocate_count = action_count(items, "relocate")?;
    let scan_id = if create_count > 0 {
        Some(
            sqlx::query_scalar(
                "INSERT INTO volund.scan_runs (library_root_id, status, finished_at, discovered_files, hashed_files) \
                 VALUES ($1, 'completed', now(), $2, $2) RETURNING id",
            )
            .bind(draft.root_id)
            .bind(create_count)
            .fetch_one(&mut **transaction)
            .await
            .map_err(database_error("create import scan record"))?,
        )
    } else {
        None
    };
    if draft.model_id.is_some() && draft.primary_item_id.is_some() {
        sqlx::query("UPDATE volund.model_source_files SET is_primary=false WHERE model_id=$1")
            .bind(model_id)
            .execute(&mut **transaction)
            .await
            .map_err(database_error("clear previous import primary source"))?;
    }
    let mut thumbnail_source_id = None;
    for (ordinal, item) in items.iter().enumerate() {
        if item.action == "skip" {
            continue;
        }
        let source_id = persist_source(transaction, draft, item, scan_id).await?;
        let primary = item.is_primary;
        if draft.thumbnail_item_id == Some(item.internal_id) {
            thumbnail_source_id = Some(source_id);
        }
        attach_source(
            transaction,
            model_id,
            source_id,
            item,
            ordinal,
            extension,
            primary,
        )
        .await?;
    }
    if let Some(source_id) = thumbnail_source_id {
        sqlx::query(
            "UPDATE volund.models SET thumbnail_kind='source-file',thumbnail_source_file_id=$2, \
             thumbnail_artifact_id=NULL WHERE id=$1",
        )
        .bind(model_id)
        .bind(source_id)
        .execute(&mut **transaction)
        .await
        .map_err(database_error("set imported thumbnail"))?;
    }
    if !extension {
        crate::tag_catalog::replace_import_tags(transaction, actor, model_id, &draft.tags)
            .await
            .map_err(import_tag_error)?;
        persist_collections(transaction, model_id, &draft.collection_ids).await?;
    }
    sqlx::query(
        "UPDATE volund.import_drafts SET status='committed',committed_at=now(),updated_at=now(), \
         result_model_public_id=$2::uuid,last_error_code=NULL WHERE id=$1",
    )
    .bind(draft.id)
    .bind(&model_public_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("complete import draft"))?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM volund.models WHERE id=$1")
        .bind(model_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error("load imported model revision"))?;
    crate::catalog_audit::record(
        transaction,
        actor,
        "model.import.commit",
        "model",
        &model_public_id,
        serde_json::json!({"modelAction":if new_model {"created"} else if extension {"extended"} else {"updated"},
            "totalFiles":items.len(),"createdFiles":create_count,"reusedFiles":reuse_count,
            "relocatedFiles":relocate_count,"revision":revision}),
    )
    .await
    .map_err(database_error("audit import commit"))?;
    Ok((model_public_id, create_count, reuse_count, relocate_count))
}

async fn attach_source(
    transaction: &mut Transaction<'_, Postgres>,
    model_id: i64,
    source_id: i64,
    item: &Item,
    ordinal: usize,
    extension: bool,
    primary: bool,
) -> Result<(), ImportCommitError> {
    let sql = if extension {
        "INSERT INTO volund.model_source_files (model_id, source_file_id, role, is_primary, ordinal) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (model_id, source_file_id) DO NOTHING"
    } else {
        "INSERT INTO volund.model_source_files (model_id, source_file_id, role, is_primary, ordinal) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (model_id, source_file_id) DO UPDATE \
         SET role = EXCLUDED.role, ordinal = EXCLUDED.ordinal, \
         is_primary = volund.model_source_files.is_primary OR EXCLUDED.is_primary"
    };
    let ordinal = i32::try_from(ordinal).map_err(|_| {
        ImportCommitError::Database("import ordinal exceeds database range".to_owned())
    })?;
    sqlx::query(sql)
        .bind(model_id)
        .bind(source_id)
        .bind(source_role(&item.category, item.is_primary))
        .bind(primary)
        .bind(ordinal)
        .execute(&mut **transaction)
        .await
        .map_err(database_error("attach imported source to model"))?;
    Ok(())
}

fn action_count(items: &[Item], action: &str) -> Result<i64, ImportCommitError> {
    i64::try_from(items.iter().filter(|item| item.action == action).count()).map_err(|_| {
        ImportCommitError::Database("import item count exceeds database range".to_owned())
    })
}

async fn persist_author(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    name: Option<&str>,
) -> Result<Option<i64>, ImportCommitError> {
    let Some(name) = name else { return Ok(None) };
    let (name, normalized) =
        normalize_identity(name, 160).map_err(ImportCommitError::BadRequest)?;
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM volund.authors WHERE normalized_name=$1 AND active ORDER BY id LIMIT 1",
    )
    .bind(&normalized)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("find import author"))?
    {
        return Ok(Some(id));
    }
    let row = sqlx::query(
        "INSERT INTO volund.authors (name,normalized_name,provenance_source) \
         VALUES ($1,$2,'import') RETURNING id,public_id::text",
    )
    .bind(name)
    .bind(normalized)
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error("create import author"))?;
    crate::catalog_audit::record(
        transaction,
        actor,
        "author.create",
        "author",
        &row.get::<String, _>(1),
        serde_json::json!({"fields":["name"],"context":"import","revision":1}),
    )
    .await
    .map_err(database_error("audit import author"))?;
    Ok(Some(row.get(0)))
}

async fn persist_model(
    transaction: &mut Transaction<'_, Postgres>,
    draft: &Draft,
    author_id: Option<i64>,
) -> Result<(i64, String, bool), ImportCommitError> {
    if let Some(model_id) = draft.model_id {
        if draft.target_action == "extend" {
            let public_id: Option<String> = sqlx::query_scalar(
                "UPDATE volund.models SET updated_at=now(),revision=revision+1 \
                 WHERE id=$1 AND revision=$2 RETURNING public_id::text",
            )
            .bind(model_id)
            .bind(draft.model_revision)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(database_error("extend import model"))?;
            return Ok((
                model_id,
                public_id.ok_or_else(|| {
                    ImportCommitError::Conflict(
                        "target model changed; review the import again".to_owned(),
                    )
                })?,
                false,
            ));
        }
        let public_id: Option<String> = sqlx::query_scalar(
            "UPDATE volund.models SET name=$2,description=$3,kind=$4, \
             author_id=COALESCE($5,author_id),license_kind=$6,license_value=$7, \
             updated_at=now(),revision=revision+1 WHERE id=$1 AND revision=$8 RETURNING public_id::text",
        )
        .bind(model_id)
        .bind(&draft.name)
        .bind(&draft.description)
        .bind(&draft.kind)
        .bind(author_id)
        .bind(&draft.license_kind)
        .bind(&draft.license_value)
        .bind(draft.model_revision)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("update import model"))?;
        Ok((
            model_id,
            public_id.ok_or_else(|| {
                ImportCommitError::Conflict(
                    "target model changed; review the import again".to_owned(),
                )
            })?,
            false,
        ))
    } else {
        let row = sqlx::query(
            "INSERT INTO volund.models (slug,name,description,kind,author_id,license_kind,license_value) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id,public_id::text",
        )
        .bind(&draft.slug)
        .bind(&draft.name)
        .bind(&draft.description)
        .bind(&draft.kind)
        .bind(author_id)
        .bind(&draft.license_kind)
        .bind(&draft.license_value)
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error("create import model"))?;
        Ok((row.get(0), row.get(1), true))
    }
}

async fn persist_source(
    transaction: &mut Transaction<'_, Postgres>,
    draft: &Draft,
    item: &Item,
    scan_id: Option<i64>,
) -> Result<i64, ImportCommitError> {
    match item.action.as_str() {
        "create" => {
            let content_id: i64 = sqlx::query_scalar(
                "INSERT INTO volund.content_objects (sha256, byte_size, detected_format) VALUES ($1, $2, $3) \
                 ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 RETURNING id",
            )
            .bind(&item.sha256)
            .bind(item.byte_size)
            .bind(detected_format(&item.target_path))
            .fetch_one(&mut **transaction)
            .await
            .map_err(database_error("persist imported content"))?;
            sqlx::query_scalar(
                "INSERT INTO volund.source_files (library_root_id, content_object_id, relative_path, \
                 filesystem_modified_at, last_seen_scan_id) VALUES ($1, $2, $3, now(), $4) RETURNING id",
            )
            .bind(draft.root_id)
            .bind(content_id)
            .bind(&item.target_path)
            .bind(scan_id.expect("create action has scan"))
            .fetch_one(&mut **transaction)
            .await
            .map_err(database_error("persist imported source"))
        }
        "relocate" => {
            let source_id = item.matched_source_id.expect("validated source match");
            let previous = item
                .matched_relative_path
                .as_deref()
                .expect("validated source path");
            sqlx::query("UPDATE volund.source_files SET relative_path = $2 WHERE id = $1")
                .bind(source_id)
                .bind(&item.target_path)
                .execute(&mut **transaction)
                .await
                .map_err(database_error("persist imported relocation"))?;
            sqlx::query(
                "INSERT INTO volund.source_file_moves (source_file_id, previous_relative_path, new_relative_path) VALUES ($1, $2, $3)",
            )
            .bind(source_id)
            .bind(previous)
            .bind(&item.target_path)
            .execute(&mut **transaction)
            .await
            .map_err(database_error("audit imported relocation"))?;
            Ok(source_id)
        }
        "reuse" => Ok(item.matched_source_id.expect("validated source match")),
        _ => Err(ImportCommitError::BadRequest(
            "invalid persisted import action".to_owned(),
        )),
    }
}

async fn persist_collections(
    transaction: &mut Transaction<'_, Postgres>,
    model_id: i64,
    collection_ids: &[i64],
) -> Result<(), ImportCommitError> {
    for (ordinal, collection_id) in collection_ids.iter().enumerate() {
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT active FROM volund.collections WHERE id=$1 FOR SHARE",
        )
        .bind(collection_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("revalidate import collection"))?;
        if active != Some(true) {
            return Err(ImportCommitError::Conflict(
                "a reviewed collection was removed; review the import again".to_owned(),
            ));
        }
        sqlx::query(
            "INSERT INTO volund.collection_models (collection_id, model_id, ordinal) \
             VALUES ($1, $2, $3) ON CONFLICT (collection_id, model_id) DO UPDATE \
             SET ordinal = EXCLUDED.ordinal",
        )
        .bind(collection_id)
        .bind(model_id)
        .bind(i32::try_from(ordinal).map_err(|_| {
            ImportCommitError::Database("collection ordinal exceeds database range".to_owned())
        })?)
        .execute(&mut **transaction)
        .await
        .map_err(database_error("attach import collection"))?;
    }
    Ok(())
}

fn detected_format(path: &str) -> Option<&'static str> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .and_then(CadFormat::from_extension)
        .map(CadFormat::as_str)
}

fn source_role(category: &str, primary: bool) -> &'static str {
    if primary {
        return "master-cad";
    }
    match category {
        "cad" => "cad",
        "mesh" => "printable-mesh",
        "document" => "document",
        "image" => "image",
        "archive" => "archive",
        _ => "other",
    }
}

fn import_tag_error(error: crate::tag_catalog::TagError) -> ImportCommitError {
    match error {
        crate::tag_catalog::TagError::BadRequest(message) => ImportCommitError::BadRequest(message),
        crate::tag_catalog::TagError::Database(message)
        | crate::tag_catalog::TagError::Conflict(message) => ImportCommitError::Database(message),
        crate::tag_catalog::TagError::NotFound => {
            ImportCommitError::Database("import tag disappeared".to_owned())
        }
        crate::tag_catalog::TagError::RevisionConflict => {
            ImportCommitError::Database("import tag revision conflict".to_owned())
        }
    }
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> ImportCommitError {
    move |error| {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            ImportCommitError::Conflict(format!("{context}: target state changed"))
        } else {
            ImportCommitError::Database(format!("cannot {context}: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_sources_receive_catalog_roles_and_supported_formats() {
        assert_eq!(source_role("cad", true), "master-cad");
        assert_eq!(source_role("mesh", false), "printable-mesh");
        assert_eq!(source_role("other", false), "other");
        assert_eq!(detected_format("CAD/part.STEP"), Some("step"));
        assert_eq!(detected_format("Docs/manual.pdf"), None);
    }
}
