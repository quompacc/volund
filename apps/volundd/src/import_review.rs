use std::collections::{HashMap, HashSet};

use sqlx::{PgPool, Postgres, Row, Transaction};
use volund_core::normalize_library_path;

use crate::api_models::{ImportReviewItemSummary, ImportReviewSummary};
use crate::session::AuthenticatedSession;

#[derive(Clone)]
struct SourceMatch {
    internal_id: i64,
    public_id: String,
    root_id: i64,
    path: String,
    sha256: String,
}

struct ReviewItem {
    internal_id: i64,
    public_id: String,
    original_path: String,
    byte_size: i64,
    category: String,
    suggested_path: String,
    is_primary: bool,
    sha256: String,
    archive_container: bool,
    resolution: Option<String>,
    resolution_path: Option<String>,
    is_thumbnail: bool,
}

struct PlannedItems {
    counts: HashMap<&'static str, i64>,
    new_bytes: i64,
    saved_bytes: i64,
    items: Vec<ImportReviewItemSummary>,
}

/// Persist a deterministic deduplication and target-path review for one upload.
///
/// # Errors
///
/// Refuses incomplete drafts and reports missing drafts or database failures.
#[allow(clippy::too_many_lines)] // One transaction derives and persists the complete deterministic plan.
pub async fn review_import(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    draft_id: &str,
) -> Result<ImportReviewSummary, ImportReviewError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin import review"))?;
    let draft = sqlx::query(
        "SELECT id,public_id::text,suggested_model_name,suggested_slug,model_kind,total_files, \
         target_library_root_id,target_action,target_model_id \
         FROM volund.import_drafts WHERE public_id::text=$1 AND owner_user_id=$2 \
         AND status IN ('uploaded','review_ready','reviewed','failed') \
         AND configured_at IS NOT NULL AND expires_at>now() FOR UPDATE",
    )
    .bind(draft_id)
    .bind(actor.database_user_id())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("load import review"))?
    .ok_or_else(|| ImportReviewError::NotFound("unknown configured import draft".to_owned()))?;
    let internal_draft_id: i64 = draft.get(0);
    let model_name: String = draft.get(2);
    let slug: String = draft.get(3);
    let kind: String = draft.get::<Option<String>, _>(4).ok_or_else(|| {
        ImportReviewError::BadRequest("import metadata must be configured first".to_owned())
    })?;
    let total_files: i32 = draft.get(5);
    let requested_root_id: i64 = draft.get::<Option<i64>, _>(6).ok_or_else(|| {
        ImportReviewError::BadRequest("target library must be configured first".to_owned())
    })?;
    let target_action: String = draft
        .get::<Option<String>, _>(7)
        .unwrap_or_else(|| "create".to_owned());
    let target_model_id: Option<i64> = draft.get(8);
    ensure_all_uploaded(&mut transaction, internal_draft_id, total_files).await?;
    let items = load_review_items(&mut transaction, internal_draft_id).await?;
    let (root_id, root_key, root_name) = choose_root(&mut transaction, requested_root_id).await?;
    let primary_source = find_primary_source(&mut transaction, internal_draft_id, root_id).await?;
    let base_directory = choose_base_directory(root_id, primary_source.as_ref(), &slug, &kind)?;
    let hash_matches = load_hash_matches(&mut transaction, internal_draft_id, root_id).await?;
    let target_paths: Vec<String> = items
        .iter()
        .map(|item| {
            item.resolution_path
                .clone()
                .map_or_else(|| planned_path(&base_directory, &item.suggested_path), Ok)
        })
        .collect::<Result<_, _>>()?;
    let occupied = load_occupied_paths(&mut transaction, root_id, &target_paths).await?;
    let existing_model_id = if let Some(model_id) = target_model_id {
        Some(
            sqlx::query_scalar("SELECT public_id::text FROM volund.models WHERE id=$1")
                .bind(model_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(database_error("load target model"))?,
        )
    } else {
        if find_model(&mut transaction, &slug).await?.is_some() {
            return Err(ImportReviewError::BadRequest(
                "a model already uses this slug; explicitly choose that model for update or rename the new model".to_owned(),
            ));
        }
        None
    };

    let planned = plan_items(
        &mut transaction,
        root_id,
        items,
        target_paths,
        &hash_matches,
        &occupied,
    )
    .await?;
    persist_draft_plan(
        &mut transaction,
        internal_draft_id,
        root_id,
        target_model_id,
        &base_directory,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit import review"))?;
    Ok(ImportReviewSummary {
        draft_id: draft_id.to_owned(),
        model_name,
        model_action: target_action,
        existing_model_id,
        root_key,
        root_name,
        base_directory,
        create_files: *planned.counts.get("create").unwrap_or(&0),
        reuse_files: *planned.counts.get("reuse").unwrap_or(&0),
        relocate_files: *planned.counts.get("relocate").unwrap_or(&0),
        conflicts: *planned.counts.get("conflict").unwrap_or(&0),
        skip_files: *planned.counts.get("skip").unwrap_or(&0),
        new_bytes: planned.new_bytes,
        saved_bytes: planned.saved_bytes,
        items: planned.items,
    })
}

async fn ensure_all_uploaded(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
    total_files: i32,
) -> Result<(), ImportReviewError> {
    let uploaded: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.import_draft_items \
         WHERE import_draft_id = $1 AND upload_status = 'uploaded' \
         AND (category <> 'archive' OR archive_expanded_at IS NOT NULL)",
    )
    .bind(draft_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error("count uploaded import items"))?;
    if uploaded != i64::from(total_files) {
        return Err(ImportReviewError::BadRequest(format!(
            "all {total_files} files must be uploaded and archives expanded before review; received {uploaded}"
        )));
    }
    Ok(())
}

async fn find_model(
    transaction: &mut Transaction<'_, Postgres>,
    slug: &str,
) -> Result<Option<(i64, String)>, ImportReviewError> {
    sqlx::query("SELECT id, public_id::text FROM volund.models WHERE slug = $1")
        .bind(slug)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error("find import model"))
        .map(|row| row.map(|model| (model.get(0), model.get(1))))
}

async fn persist_draft_plan(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
    root_id: i64,
    model_id: Option<i64>,
    base_directory: &str,
) -> Result<(), ImportReviewError> {
    sqlx::query(
        "UPDATE volund.import_drafts SET target_library_root_id=$2,target_model_id=$3, \
         planned_base_directory=$4,reviewed_at=now(),status='reviewed',updated_at=now() WHERE id=$1",
    )
    .bind(draft_id)
    .bind(root_id)
    .bind(model_id)
    .bind(base_directory)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("persist import review"))?;
    Ok(())
}

async fn plan_items(
    transaction: &mut Transaction<'_, Postgres>,
    root_id: i64,
    items: Vec<ReviewItem>,
    target_paths: Vec<String>,
    hash_matches: &HashMap<i64, SourceMatch>,
    occupied: &HashMap<String, SourceMatch>,
) -> Result<PlannedItems, ImportReviewError> {
    let mut planned = PlannedItems {
        counts: HashMap::new(),
        new_bytes: 0,
        saved_bytes: 0,
        items: Vec::with_capacity(items.len()),
    };
    let mut relocated = HashSet::new();
    for (item, target_path) in items.into_iter().zip(target_paths) {
        let (mut action, mut matched) =
            if item.archive_container || item.resolution.as_deref() == Some("skip") {
                (
                    if item.is_primary || item.is_thumbnail {
                        "conflict"
                    } else {
                        "skip"
                    },
                    None,
                )
            } else if item.resolution.as_deref() == Some("create") {
                if let Some(target) = occupied.get(&target_path) {
                    ("conflict", Some(target))
                } else {
                    ("create", None)
                }
            } else if item.resolution.as_deref() == Some("reuse") {
                let source = occupied
                    .get(&target_path)
                    .filter(|source| source.sha256 == item.sha256)
                    .or_else(|| hash_matches.get(&item.internal_id));
                source.map_or(("conflict", None), |source| ("reuse", Some(source)))
            } else {
                decide_action(
                    root_id,
                    &item.sha256,
                    hash_matches.get(&item.internal_id),
                    occupied.get(&target_path),
                )
            };
        if action == "relocate"
            && !relocated.insert(matched.expect("relocation source").internal_id)
        {
            // Separate uploaded filenames need separate catalog sources, not two moves of one ID.
            action = "create";
            matched = None;
        }
        *planned.counts.entry(action).or_insert(0) += 1;
        if action == "create" {
            planned.new_bytes += item.byte_size;
        } else if matches!(action, "reuse" | "relocate") {
            planned.saved_bytes += item.byte_size;
        }
        persist_item_plan(
            transaction,
            item.internal_id,
            action,
            &target_path,
            matched.map(|source| source.internal_id),
        )
        .await?;
        planned.items.push(ImportReviewItemSummary {
            id: item.public_id,
            original_path: item.original_path,
            category: item.category,
            byte_size: item.byte_size,
            sha256: item.sha256,
            action: action.to_owned(),
            target_path,
            existing_file_id: matched.map(|source| source.public_id.clone()),
            existing_path: matched.map(|source| source.path.clone()),
            is_primary: item.is_primary,
        });
    }
    Ok(planned)
}

#[derive(Debug)]
pub enum ImportReviewError {
    BadRequest(String),
    NotFound(String),
    Database(String),
}

pub use crate::import_resolution::resolve_item;

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> ImportReviewError {
    move |error| ImportReviewError::Database(format!("cannot {context}: {error}"))
}

async fn load_review_items(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
) -> Result<Vec<ReviewItem>, ImportReviewError> {
    sqlx::query(
        "SELECT id, public_id::text, original_path, byte_size, category, \
         suggested_relative_path,COALESCE(id=(SELECT primary_item_id FROM volund.import_drafts WHERE id=$1),false),sha256,archive_expanded_at IS NOT NULL, \
         resolution,resolution_relative_path,COALESCE(id=(SELECT thumbnail_item_id FROM volund.import_drafts WHERE id=$1),false) \
         FROM volund.import_draft_items WHERE import_draft_id = $1 ORDER BY id",
    )
    .bind(draft_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error("load review items"))?
    .into_iter()
    .map(|row| {
        Ok(ReviewItem {
            internal_id: row.get(0),
            public_id: row.get(1),
            original_path: row.get(2),
            byte_size: row.get(3),
            category: row.get(4),
            suggested_path: row.get(5),
            is_primary: row.get(6),
            sha256: row.get::<Option<String>, _>(7).ok_or_else(|| {
                ImportReviewError::BadRequest("uploaded item has no SHA-256".to_owned())
            })?,
            archive_container: row.get(8),
            resolution: row.get(9),
            resolution_path: row.get(10),
            is_thumbnail: row.get(11),
        })
    })
    .collect()
}

async fn find_primary_source(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
    root_id: i64,
) -> Result<Option<SourceMatch>, ImportReviewError> {
    sqlx::query(
        "SELECT s.id, s.public_id::text, s.library_root_id, s.relative_path, c.sha256 \
         FROM volund.import_draft_items i JOIN volund.content_objects c ON c.sha256 = i.sha256 \
         JOIN volund.source_files s ON s.content_object_id = c.id AND s.missing_at IS NULL \
         JOIN volund.import_drafts d ON d.id=i.import_draft_id AND d.primary_item_id=i.id \
         WHERE i.import_draft_id = $1 AND s.library_root_id = $2 \
         ORDER BY s.id LIMIT 1",
    )
    .bind(draft_id)
    .bind(root_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("find primary source"))
    .map(|row| row.map(|value| source_match(&value)))
}

async fn choose_root(
    transaction: &mut Transaction<'_, Postgres>,
    root_id: i64,
) -> Result<(i64, String, String), ImportReviewError> {
    let row =
        sqlx::query("SELECT id, root_key, display_name FROM volund.library_roots WHERE id = $1")
            .bind(root_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(database_error("choose import root"))?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

fn choose_base_directory(
    root_id: i64,
    primary: Option<&SourceMatch>,
    slug: &str,
    kind: &str,
) -> Result<String, ImportReviewError> {
    if let Some(primary) = primary.filter(|source| source.root_id == root_id) {
        let parent = primary
            .path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let base = if parent.is_empty() {
            slug.to_owned()
        } else {
            format!("{parent}/{slug}")
        };
        return normalize_library_path(&base)
            .map_err(|message| ImportReviewError::BadRequest(message.to_owned()));
    }
    let bucket = match kind {
        "part" => "Bauteile",
        "assembly" => "Baugruppen",
        _ => "Projekte",
    };
    let base = format!("{bucket}/{slug}");
    normalize_library_path(&base)
        .map_err(|message| ImportReviewError::BadRequest(message.to_owned()))
}

async fn load_hash_matches(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
    root_id: i64,
) -> Result<HashMap<i64, SourceMatch>, ImportReviewError> {
    let rows = sqlx::query(
        "SELECT i.id, s.id, s.public_id::text, s.library_root_id, s.relative_path, c.sha256 \
         FROM volund.import_draft_items i JOIN volund.content_objects c ON c.sha256 = i.sha256 \
         JOIN volund.source_files s ON s.content_object_id = c.id AND s.missing_at IS NULL \
         WHERE i.import_draft_id = $1 ORDER BY i.id, (s.library_root_id = $2) DESC, s.id",
    )
    .bind(draft_id)
    .bind(root_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error("load existing content matches"))?;
    let mut matches = HashMap::new();
    for row in rows {
        matches
            .entry(row.get(0))
            .or_insert_with(|| source_match_from(&row, 1));
    }
    Ok(matches)
}

async fn load_occupied_paths(
    transaction: &mut Transaction<'_, Postgres>,
    root_id: i64,
    paths: &[String],
) -> Result<HashMap<String, SourceMatch>, ImportReviewError> {
    let rows = sqlx::query(
        "SELECT s.id, s.public_id::text, s.library_root_id, s.relative_path, c.sha256 \
         FROM volund.source_files s JOIN volund.content_objects c ON c.id = s.content_object_id \
         WHERE s.library_root_id = $1 AND s.missing_at IS NULL AND s.relative_path = ANY($2)",
    )
    .bind(root_id)
    .bind(paths)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error("load occupied import paths"))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let source = source_match(&row);
            (source.path.clone(), source)
        })
        .collect())
}

fn source_match(row: &sqlx::postgres::PgRow) -> SourceMatch {
    source_match_from(row, 0)
}

fn source_match_from(row: &sqlx::postgres::PgRow, offset: usize) -> SourceMatch {
    SourceMatch {
        internal_id: row.get(offset),
        public_id: row.get(offset + 1),
        root_id: row.get(offset + 2),
        path: row.get(offset + 3),
        sha256: row.get(offset + 4),
    }
}

fn planned_path(base: &str, suggested: &str) -> Result<String, ImportReviewError> {
    // The first segment is the slug captured when the draft was created. The user may
    // rename the model before review, so it must not be compared with the current slug.
    let suffix = suggested
        .split_once('/')
        .map_or(suggested, |(_, remainder)| remainder);
    let mut parts: Vec<&str> = suffix.split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 1].eq_ignore_ascii_case(parts[parts.len() - 2]) {
        parts.remove(parts.len() - 2);
    }
    normalize_library_path(&format!("{base}/{}", parts.join("/")))
        .map_err(|message| ImportReviewError::BadRequest(message.to_owned()))
}

fn decide_action<'a>(
    root_id: i64,
    sha256: &str,
    hash_match: Option<&'a SourceMatch>,
    target_match: Option<&'a SourceMatch>,
) -> (&'static str, Option<&'a SourceMatch>) {
    if let Some(target) = target_match {
        return if target.sha256 == sha256 {
            ("reuse", Some(target))
        } else {
            ("conflict", Some(target))
        };
    }
    match hash_match {
        Some(source) if source.root_id == root_id => ("relocate", Some(source)),
        Some(source) => ("reuse", Some(source)),
        None => ("create", None),
    }
}

async fn persist_item_plan(
    transaction: &mut Transaction<'_, Postgres>,
    item_id: i64,
    action: &str,
    target_path: &str,
    source_id: Option<i64>,
) -> Result<(), ImportReviewError> {
    sqlx::query(
        "UPDATE volund.import_draft_items SET planned_action = $2, planned_relative_path = $3, \
         matched_source_file_id = $4 WHERE id = $1",
    )
    .bind(item_id)
    .bind(action)
    .bind(target_path)
    .bind(source_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("persist import item review"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_paths_remove_redundant_filename_directories() {
        assert_eq!(
            planned_path(
                "Test/Baugruppen/voron",
                "initial-slug/CAD/main.step/main.step"
            )
            .unwrap(),
            "Test/Baugruppen/voron/CAD/main.step"
        );
        assert!(planned_path("../escape", "initial-slug/CAD/main.step").is_err());
    }

    #[test]
    fn review_actions_prefer_target_conflicts_and_same_root_relocation() {
        let source = SourceMatch {
            internal_id: 1,
            public_id: "one".into(),
            root_id: 4,
            path: "old.step".into(),
            sha256: "a".repeat(64),
        };
        let conflict = SourceMatch {
            sha256: "b".repeat(64),
            ..source.clone()
        };
        assert_eq!(
            decide_action(4, &source.sha256, Some(&source), None).0,
            "relocate"
        );
        assert_eq!(
            decide_action(4, &source.sha256, Some(&source), Some(&conflict)).0,
            "conflict"
        );
        assert_eq!(decide_action(4, &source.sha256, None, None).0, "create");
    }
}
