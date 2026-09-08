use std::collections::HashSet;
use std::path::Path;

use sqlx::{PgPool, Postgres, Row, Transaction};
use unicode_normalization::UnicodeNormalization;
use volund_core::normalize_library_path;

use crate::api_models::{
    ImportDraftItemSummary, ImportDraftSummary, ImportManifestEntry, ImportPreviewRequest,
};
use crate::session::AuthenticatedSession;

const MAX_IMPORT_FILES: usize = 10_000;
const MAX_IMPORT_BYTES: i64 = 10 * 1024 * 1024 * 1024;
const MAX_IMPORT_ITEM_BYTES: i64 = 2 * 1024 * 1024 * 1024;
const MAX_IMPORT_PATH_CHARS: usize = 1024;
const MAX_IMPORT_PATH_DEPTH: usize = 32;

struct PlannedItem {
    original_path: String,
    byte_size: i64,
    category: &'static str,
    suggested_relative_path: String,
    is_primary_candidate: bool,
}

/// Validate, classify, persist, and return a metadata-only import proposal.
///
/// # Errors
///
/// Returns an error when the manifest is empty, unsafe, duplicated, oversized,
/// or cannot be persisted. No source bytes or library paths are changed.
pub async fn preview_import(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    request: ImportPreviewRequest,
) -> Result<ImportDraftSummary, ImportPlanError> {
    let policy = crate::import_policy::load(pool).await.map_err(|error| {
        ImportPlanError::Database(format!("cannot load import policy: {error:?}"))
    })?;
    let source_name = request.source_name.trim();
    if source_name.is_empty() {
        return Err(ImportPlanError::BadRequest(
            "sourceName must not be empty".to_owned(),
        ));
    }
    if request.entries.is_empty() || request.entries.len() > MAX_IMPORT_FILES {
        return Err(ImportPlanError::BadRequest(format!(
            "entries must contain between 1 and {MAX_IMPORT_FILES} files"
        )));
    }
    let normalized = normalize_entries(request.entries).map_err(ImportPlanError::BadRequest)?;
    let total_bytes = normalized
        .iter()
        .try_fold(0_i64, |total, item| {
            total
                .checked_add(item.byte_size)
                .ok_or_else(|| "total import size is out of range".to_owned())
        })
        .map_err(ImportPlanError::BadRequest)?;
    if total_bytes > MAX_IMPORT_BYTES {
        return Err(ImportPlanError::BadRequest(
            "import manifest exceeds the 10 GB limit".to_owned(),
        ));
    }
    let suggested_model_name = model_name(source_name);
    let suggested_slug = slugify(&suggested_model_name).map_err(ImportPlanError::BadRequest)?;
    let mut items = plan_items(normalized, &suggested_slug);
    mark_primary_candidate(&mut items);
    let mut transaction = pool.begin().await.map_err(|error| {
        ImportPlanError::Database(format!("cannot begin import draft: {error}"))
    })?;
    let reserved = crate::import_capacity::lock_and_used(&mut transaction)
        .await
        .map_err(|error| {
            ImportPlanError::Database(format!("cannot inspect import capacity: {error}"))
        })?;
    if reserved.saturating_add(total_bytes) > policy.incoming_capacity_bytes {
        return Err(ImportPlanError::BadRequest(
            "incoming capacity would be exceeded".to_owned(),
        ));
    }
    persist_draft(
        transaction,
        source_name,
        &suggested_model_name,
        &suggested_slug,
        total_bytes,
        items,
        actor,
        policy.retention_days,
    )
    .await
}

#[derive(Debug)]
pub enum ImportPlanError {
    BadRequest(String),
    NotFound(String),
    Database(String),
}

fn normalize_entries(
    entries: Vec<ImportManifestEntry>,
) -> Result<Vec<ImportManifestEntry>, String> {
    let mut seen = HashSet::with_capacity(entries.len());
    entries
        .into_iter()
        .map(|entry| {
            if !(0..=MAX_IMPORT_ITEM_BYTES).contains(&entry.byte_size) {
                return Err("file size exceeds the 2 GiB item limit".to_owned());
            }
            let path = normalize_library_path(&entry.path)?.clone();
            if path.chars().count() > MAX_IMPORT_PATH_CHARS
                || path.split('/').count() > MAX_IMPORT_PATH_DEPTH
            {
                return Err("import path exceeds length or depth limits".to_owned());
            }
            let collision_key = path.nfkc().collect::<String>().to_lowercase();
            if !seen.insert(collision_key) {
                return Err(format!("duplicate import path: {path}"));
            }
            Ok(ImportManifestEntry {
                path,
                byte_size: entry.byte_size,
            })
        })
        .collect()
}

fn model_name(source_name: &str) -> String {
    let file_name = Path::new(source_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(source_name);
    Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .trim()
        .to_owned()
}

pub(crate) fn slugify(value: &str) -> Result<String, String> {
    let expanded = value
        .to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss");
    let mut slug = String::new();
    let mut separator = false;
    for character in expanded.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        Err("sourceName must contain letters or numbers".to_owned())
    } else {
        Ok(slug)
    }
}

fn plan_items(entries: Vec<ImportManifestEntry>, slug: &str) -> Vec<PlannedItem> {
    let common_root = common_root(&entries);
    entries
        .into_iter()
        .map(|entry| {
            let relative = common_root
                .as_deref()
                .and_then(|root| entry.path.strip_prefix(&format!("{root}/")))
                .unwrap_or(&entry.path);
            let category = classify_path(&entry.path);
            let relative = strip_category_directory(relative, category);
            PlannedItem {
                suggested_relative_path: format!(
                    "{slug}/{}/{relative}",
                    category_directory(category)
                ),
                original_path: entry.path,
                byte_size: entry.byte_size,
                category,
                is_primary_candidate: false,
            }
        })
        .collect()
}

fn strip_category_directory<'a>(path: &'a str, category: &str) -> &'a str {
    let Some((first, remainder)) = path.split_once('/') else {
        return path;
    };
    let normalized = first.to_ascii_lowercase();
    let matches = match category {
        "cad" => matches!(normalized.as_str(), "cad" | "step" | "source" | "sources"),
        "mesh" => matches!(
            normalized.as_str(),
            "mesh" | "meshes" | "stl" | "stls" | "3mf"
        ),
        "document" => matches!(normalized.as_str(), "docs" | "documents" | "dokumente"),
        "image" => matches!(normalized.as_str(), "images" | "bilder" | "photos"),
        _ => false,
    };
    if matches { remainder } else { path }
}

fn common_root(entries: &[ImportManifestEntry]) -> Option<String> {
    let first = entries.first()?.path.split('/').next()?;
    if entries.len() > 1
        && entries
            .iter()
            .all(|entry| entry.path.starts_with(&format!("{first}/")))
    {
        Some(first.to_owned())
    } else {
        None
    }
}

fn classify_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "step" | "stp" | "iges" | "igs" | "brep" | "f3d" => "cad",
        "stl" | "3mf" | "obj" | "ply" | "gltf" | "glb" => "mesh",
        "pdf" | "xls" | "xlsx" | "csv" | "txt" | "md" | "doc" | "docx" => "document",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" => "image",
        "zip" | "7z" | "tar" | "gz" => "archive",
        _ => "other",
    }
}

fn category_directory(category: &str) -> &'static str {
    match category {
        "cad" => "CAD",
        "mesh" => "Meshes",
        "document" => "Dokumente",
        "image" => "Bilder",
        "archive" => "Archive",
        _ => "Sonstiges",
    }
}

fn mark_primary_candidate(items: &mut [PlannedItem]) {
    let primary = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.category == "cad")
        .max_by_key(|(_, item)| primary_candidate_rank(item))
        .map(|(index, _)| index);
    if let Some(index) = primary {
        items[index].is_primary_candidate = true;
    }
}

fn primary_candidate_rank(item: &PlannedItem) -> (u8, bool, i64) {
    let path = item.original_path.to_ascii_lowercase();
    let format = path.rsplit('.').next().unwrap_or_default();
    let supported = match format {
        "step" | "stp" => 4,
        "iges" | "igs" => 3,
        "brep" => 2,
        _ => 1,
    };
    (supported, path.contains("assembly"), item.byte_size)
}

#[allow(clippy::too_many_arguments)]
async fn persist_draft(
    mut transaction: Transaction<'_, Postgres>,
    source_name: &str,
    model_name: &str,
    slug: &str,
    total_bytes: i64,
    items: Vec<PlannedItem>,
    actor: &AuthenticatedSession,
    retention_days: i64,
) -> Result<ImportDraftSummary, ImportPlanError> {
    let total_files = i32::try_from(items.len())
        .map_err(|_| ImportPlanError::BadRequest("too many import files".to_owned()))?;
    let row = sqlx::query(
        "INSERT INTO volund.import_drafts \
         (source_name,display_name,suggested_model_name,suggested_slug,total_files,total_bytes,owner_user_id,expires_at) \
         VALUES ($1,$1,$2,$3,$4,$5,$6,now()+make_interval(days=>$7::int)) RETURNING id, public_id::text",
    )
    .bind(source_name)
    .bind(model_name)
    .bind(slug)
    .bind(total_files)
    .bind(total_bytes)
    .bind(actor.database_user_id())
    .bind(i32::try_from(retention_days).map_err(|_| ImportPlanError::BadRequest("retention is out of range".to_owned()))?)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ImportPlanError::Database(format!("cannot persist import draft: {error}")))?;
    let draft_id: i64 = row.get(0);
    let public_id: String = row.get(1);
    let mut item_ids = Vec::with_capacity(items.len());
    for item in &items {
        let public_id: String = sqlx::query_scalar(
            "INSERT INTO volund.import_draft_items \
             (import_draft_id, original_path, byte_size, category, \
              suggested_relative_path, is_primary_candidate) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING public_id::text",
        )
        .bind(draft_id)
        .bind(&item.original_path)
        .bind(item.byte_size)
        .bind(item.category)
        .bind(&item.suggested_relative_path)
        .bind(item.is_primary_candidate)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| {
            ImportPlanError::Database(format!("cannot persist import item: {error}"))
        })?;
        item_ids.push(public_id);
    }
    crate::catalog_audit::record(
        &mut transaction,
        actor,
        "import.create",
        "import-draft",
        &public_id,
        serde_json::json!({"totalFiles":total_files,"totalBytes":total_bytes}),
    )
    .await
    .map_err(|error| ImportPlanError::Database(format!("cannot audit import draft: {error}")))?;
    transaction.commit().await.map_err(|error| {
        ImportPlanError::Database(format!("cannot commit import draft: {error}"))
    })?;
    Ok(ImportDraftSummary {
        id: public_id,
        source_name: source_name.to_owned(),
        suggested_model_name: model_name.to_owned(),
        suggested_slug: slug.to_owned(),
        total_files: i64::from(total_files),
        total_bytes,
        items: items
            .into_iter()
            .zip(item_ids)
            .map(|(item, id)| ImportDraftItemSummary {
                id,
                original_path: item.original_path,
                byte_size: item.byte_size,
                category: item.category.to_owned(),
                suggested_relative_path: item.suggested_relative_path,
                is_primary_candidate: item.is_primary_candidate,
                upload_status: "pending".to_owned(),
            })
            .collect(),
    })
}

#[cfg(test)]
#[path = "import_planner_tests.rs"]
mod tests;
