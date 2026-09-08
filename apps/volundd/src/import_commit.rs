use std::path::{Path, PathBuf};

use sqlx::{PgPool, Postgres, Row, Transaction};
use tokio::fs;

use crate::api_models::ImportCommitSummary;
use crate::import_files::{ImportFiles, publish_files};
use crate::import_persistence::persist_import;
use crate::session::AuthenticatedSession;

const LIBRARY_LOCK_NAMESPACE: i64 = 6_219_273_558_347_087_872;

pub(crate) struct Draft {
    pub(crate) id: i64,
    pub(crate) public_id: String,
    pub(crate) name: String,
    pub(crate) slug: String,
    pub(crate) kind: String,
    pub(crate) description: String,
    pub(crate) author_name: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) collection_ids: Vec<i64>,
    pub(crate) root_id: i64,
    pub(crate) root_path: PathBuf,
    pub(crate) model_id: Option<i64>,
    pub(crate) model_revision: Option<i64>,
    pub(crate) total_files: i32,
    pub(crate) license_kind: String,
    pub(crate) license_value: Option<String>,
    pub(crate) primary_item_id: Option<i64>,
    pub(crate) thumbnail_item_id: Option<i64>,
    pub(crate) target_action: String,
}

pub(crate) struct Item {
    pub(crate) internal_id: i64,
    pub(crate) public_id: String,
    pub(crate) byte_size: i64,
    pub(crate) category: String,
    pub(crate) sha256: String,
    pub(crate) action: String,
    pub(crate) target_path: String,
    pub(crate) matched_source_id: Option<i64>,
    pub(crate) matched_root_path: Option<PathBuf>,
    pub(crate) matched_relative_path: Option<String>,
    pub(crate) is_primary: bool,
}

/// Publish one reviewed import into its library and attach every source to its model.
///
/// Files are staged without overwrites; original paths survive until commit.
/// The review decisions and every content hash are revalidated while the
/// library's advisory lock and draft row lock are held.
///
/// # Errors
///
/// Refuses stale, incomplete, conflicting, or already committed plans and reports
/// storage/database failures. Before commit, staged links are rolled back;
/// ambiguous commit outcomes preserve files and may require reconciliation.
pub async fn commit_import(
    pool: &PgPool,
    incoming_root: &Path,
    draft_id: &str,
    actor: &AuthenticatedSession,
) -> Result<ImportCommitSummary, ImportCommitError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin import commit"))?;
    if let Some(result) = load_committed(pool, draft_id, actor.database_user_id()).await? {
        drop(transaction);
        crate::import_cleanup::finish_staging(pool, incoming_root, draft_id)
            .await
            .map_err(|error| ImportCommitError::Storage(format!("import committed; {error}")))?;
        return Ok(result);
    }
    let draft = load_draft(&mut transaction, draft_id, actor.database_user_id()).await?;
    acquire_library_lock(&mut transaction, draft.root_id).await?;
    lock_target_model(&mut transaction, &draft).await?;
    // Use wall-clock time after all blocking locks, not transaction-start now().
    let unexpired: bool = sqlx::query_scalar(
        "SELECT expires_at > clock_timestamp() FROM volund.import_drafts WHERE id=$1",
    )
    .bind(draft.id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("check import expiry"))?;
    if !unexpired {
        return Err(ImportCommitError::NotFound(
            "import draft has expired".to_owned(),
        ));
    }
    sqlx::query("UPDATE volund.import_drafts SET status='committing',commit_started_at=now(),updated_at=now() WHERE id=$1")
        .bind(draft.id).execute(&mut *transaction).await.map_err(database_error("start import commit"))?;
    let mut items = load_items(&mut transaction, draft.id).await?;
    for item in &mut items {
        item.is_primary = draft.primary_item_id == Some(item.internal_id);
    }
    validate_plan(&draft, &items)?;
    let library_root = fs::canonicalize(&draft.root_path)
        .await
        .map_err(storage_error("resolve import library root"))?;
    let staging_directory = incoming_root.join(&draft.public_id);
    let mut changes = ImportFiles::default();

    if let Err(error) = publish_files(
        &mut transaction,
        &library_root,
        &staging_directory,
        &items,
        &mut changes,
    )
    .await
    {
        drop(changes);
        transaction.rollback().await.ok();
        record_failure(pool, draft_id, "commit_revalidation_failed").await;
        return Err(error);
    }
    let persisted = persist_import(&mut transaction, actor, &draft, &items).await;
    let (model_id, created, reused, relocated) = match persisted {
        Ok(result) => result,
        Err(error) => {
            drop(changes);
            transaction.rollback().await.ok();
            record_failure(pool, draft_id, "commit_persistence_failed").await;
            return Err(error);
        }
    };
    changes.preserve();
    if let Err(error) = transaction.commit().await {
        record_failure(pool, draft_id, "commit_transaction_failed").await;
        return Err(database_error("commit import transaction")(error));
    }
    for source in &changes.relocated {
        crate::source_cleanup::finish(pool, *source)
            .await
            .map_err(|error| {
                ImportCommitError::Storage(format!(
                    "import committed; source cleanup pending retry: {error}"
                ))
            })?;
    }
    crate::import_cleanup::finish_staging(pool, incoming_root, draft_id)
        .await
        .map_err(|error| ImportCommitError::Storage(format!("import committed; {error}")))?;
    Ok(ImportCommitSummary {
        draft_id: draft.public_id,
        model_id,
        model_name: draft.name,
        total_files: i64::from(draft.total_files),
        created_files: created,
        reused_files: reused,
        relocated_files: relocated,
        status: "committed".to_owned(),
    })
}

async fn record_failure(pool: &PgPool, draft_id: &str, code: &str) {
    let _ = sqlx::query(
        "UPDATE volund.import_drafts SET status='failed',last_error_code=$2,updated_at=now() \
         WHERE public_id::text=$1 AND status IN ('reviewed','committing')",
    )
    .bind(draft_id)
    .bind(code)
    .execute(pool)
    .await;
}

#[derive(Debug)]
pub enum ImportCommitError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Busy(String),
    Storage(String),
    Database(String),
}

async fn load_draft(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: &str,
    owner_user_id: i64,
) -> Result<Draft, ImportCommitError> {
    let row = sqlx::query(
        "SELECT d.id, d.public_id::text, d.suggested_model_name, d.suggested_slug, \
         d.model_kind, d.description, d.author_name, d.tags, \
         COALESCE((SELECT array_agg(collection.collection_id ORDER BY collection.ordinal) \
           FROM volund.import_draft_collections collection WHERE collection.import_draft_id = d.id), \
           ARRAY[]::bigint[]), d.target_library_root_id, root.filesystem_path, \
         d.target_model_id,d.total_files,d.reviewed_at IS NOT NULL,d.target_model_revision, \
         d.license_kind,d.license_value,d.primary_item_id,d.thumbnail_item_id,d.target_action \
         FROM volund.import_drafts d LEFT JOIN volund.library_roots root \
         ON root.id = d.target_library_root_id WHERE d.public_id::text = $1 \
         AND d.owner_user_id=$2 AND d.status IN ('reviewed','failed') FOR UPDATE OF d",
    )
    .bind(draft_id)
    .bind(owner_user_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error("load import draft"))?
    .ok_or_else(|| ImportCommitError::NotFound("unknown active import draft".to_owned()))?;
    if !row.get::<bool, _>(13) {
        return Err(ImportCommitError::BadRequest(
            "import must be reviewed immediately before confirmation".to_owned(),
        ));
    }
    Ok(Draft {
        id: row.get(0),
        public_id: row.get(1),
        name: row.get(2),
        slug: row.get(3),
        kind: row.get::<Option<String>, _>(4).ok_or_else(|| {
            ImportCommitError::BadRequest("import metadata is incomplete".to_owned())
        })?,
        description: row.get(5),
        author_name: row.get(6),
        tags: row.get(7),
        collection_ids: row.get(8),
        root_id: row.get::<Option<i64>, _>(9).ok_or_else(|| {
            ImportCommitError::BadRequest("import has no reviewed library root".to_owned())
        })?,
        root_path: PathBuf::from(row.get::<Option<String>, _>(10).ok_or_else(|| {
            ImportCommitError::BadRequest("reviewed library root no longer exists".to_owned())
        })?),
        model_id: row.get(11),
        total_files: row.get(12),
        model_revision: row.get(14),
        license_kind: row.get(15),
        license_value: row.get(16),
        primary_item_id: row.get(17),
        thumbnail_item_id: row.get(18),
        target_action: row.get(19),
    })
}

async fn lock_target_model(
    transaction: &mut Transaction<'_, Postgres>,
    draft: &Draft,
) -> Result<(), ImportCommitError> {
    let Some(model_id) = draft.model_id else {
        return Ok(());
    };
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM volund.models WHERE id=$1 FOR UPDATE")
            .bind(model_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(database_error("lock target model"))?;
    if revision != draft.model_revision {
        return Err(ImportCommitError::Conflict(
            "target model changed; review the import again".to_owned(),
        ));
    }
    Ok(())
}

async fn load_committed(
    pool: &PgPool,
    draft_id: &str,
    owner_user_id: i64,
) -> Result<Option<ImportCommitSummary>, ImportCommitError> {
    let row = sqlx::query(
        "SELECT d.public_id::text,m.public_id::text,d.suggested_model_name,d.total_files::bigint, \
         count(*) FILTER (WHERE i.planned_action='create')::bigint, \
         count(*) FILTER (WHERE i.planned_action='reuse')::bigint, \
         count(*) FILTER (WHERE i.planned_action='relocate')::bigint \
         FROM volund.import_drafts d JOIN volund.models m ON m.public_id=d.result_model_public_id \
         JOIN volund.import_draft_items i ON i.import_draft_id=d.id \
         WHERE d.public_id::text=$1 AND d.owner_user_id=$2 AND d.status='committed' \
         GROUP BY d.id,m.public_id",
    )
    .bind(draft_id)
    .bind(owner_user_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error("load committed import result"))?;
    Ok(row.map(|row| ImportCommitSummary {
        draft_id: row.get(0),
        model_id: row.get(1),
        model_name: row.get(2),
        total_files: row.get(3),
        created_files: row.get(4),
        reused_files: row.get(5),
        relocated_files: row.get(6),
        status: "committed".to_owned(),
    }))
}

async fn acquire_library_lock(
    transaction: &mut Transaction<'_, Postgres>,
    root_id: i64,
) -> Result<(), ImportCommitError> {
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1 + $2)")
        .bind(LIBRARY_LOCK_NAMESPACE)
        .bind(root_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error("lock import library"))?;
    if locked {
        Ok(())
    } else {
        Err(ImportCommitError::Busy(
            "library is currently being scanned or modified".to_owned(),
        ))
    }
}

async fn load_items(
    transaction: &mut Transaction<'_, Postgres>,
    draft_id: i64,
) -> Result<Vec<Item>, ImportCommitError> {
    sqlx::query("SELECT id FROM volund.source_files WHERE id IN (SELECT matched_source_file_id FROM volund.import_draft_items WHERE import_draft_id=$1) ORDER BY id FOR UPDATE")
        .bind(draft_id).fetch_all(&mut **transaction).await.map_err(database_error("lock matched import sources"))?;
    sqlx::query(
        "SELECT item.id,item.public_id::text,item.byte_size,item.category,item.sha256, \
         item.planned_action, item.planned_relative_path, item.matched_source_file_id, \
         root.filesystem_path, source.relative_path, item.is_primary_candidate, item.upload_status, \
         (item.category <> 'archive' OR item.archive_expanded_at IS NOT NULL) \
         FROM volund.import_draft_items item LEFT JOIN volund.source_files source \
         ON source.id = item.matched_source_file_id LEFT JOIN volund.library_roots root \
         ON root.id = source.library_root_id WHERE item.import_draft_id = $1 ORDER BY item.id",
    )
    .bind(draft_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error("load import commit items"))?
    .into_iter()
    .map(|row| {
        if row.get::<String, _>(11) != "uploaded" || !row.get::<bool, _>(12) {
            return Err(ImportCommitError::BadRequest(
                "every import item must be uploaded and archives expanded".to_owned(),
            ));
        }
        Ok(Item {
            internal_id: row.get(0),
            public_id: row.get(1),
            byte_size: row.get(2),
            category: row.get(3),
            sha256: row.get::<Option<String>, _>(4).ok_or_else(|| {
                ImportCommitError::BadRequest("uploaded item has no SHA-256".to_owned())
            })?,
            action: row.get::<Option<String>, _>(5).ok_or_else(|| {
                ImportCommitError::BadRequest("import item has not been reviewed".to_owned())
            })?,
            target_path: row.get::<Option<String>, _>(6).ok_or_else(|| {
                ImportCommitError::BadRequest("import item has no target path".to_owned())
            })?,
            matched_source_id: row.get(7),
            matched_root_path: row.get::<Option<String>, _>(8).map(PathBuf::from),
            matched_relative_path: row.get(9),
            is_primary: row.get(10),
        })
    })
    .collect()
}

fn validate_plan(draft: &Draft, items: &[Item]) -> Result<(), ImportCommitError> {
    let mut relocated = std::collections::HashSet::new();
    for item in items.iter().filter(|item| item.action == "relocate") {
        if !relocated.insert(item.matched_source_id) {
            return Err(ImportCommitError::Conflict(
                "a source cannot be relocated twice; review the import again".to_owned(),
            ));
        }
    }
    let total_files = usize::try_from(draft.total_files).map_err(|_| {
        ImportCommitError::Database("stored import file count is negative".to_owned())
    })?;
    if items.len() != total_files {
        return Err(ImportCommitError::Conflict(
            "reviewed file count changed; review the import again".to_owned(),
        ));
    }
    if items.iter().any(|item| item.action == "conflict") {
        return Err(ImportCommitError::Conflict(
            "import contains unresolved target conflicts".to_owned(),
        ));
    }
    if items.iter().any(|item| {
        matches!(item.action.as_str(), "reuse" | "relocate") && item.matched_source_id.is_none()
    }) {
        return Err(ImportCommitError::Conflict(
            "a reviewed source match disappeared".to_owned(),
        ));
    }
    if items.iter().any(|item| {
        item.action == "skip"
            && (item.is_primary || draft.thumbnail_item_id == Some(item.internal_id))
    }) {
        return Err(ImportCommitError::Conflict(
            "a selected primary or thumbnail item cannot be skipped".to_owned(),
        ));
    }
    if draft.target_action == "extend" && draft.primary_item_id.is_some() {
        return Err(ImportCommitError::Conflict(
            "model extension cannot replace the primary source".to_owned(),
        ));
    }
    Ok(())
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

fn storage_error(context: &'static str) -> impl FnOnce(std::io::Error) -> ImportCommitError {
    move |error| ImportCommitError::Storage(format!("cannot {context}: {error}"))
}
