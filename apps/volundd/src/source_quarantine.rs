use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Row};
use tokio::fs;

use crate::catalog_audit;
use crate::file_hash::sha256_file;
use crate::lifecycle::{ApplyRequest, ApplySummary, LifecycleError};
use crate::session::AuthenticatedSession;

const QUARANTINE_DIRECTORY: &str = ".volund-quarantine";
const DEFAULT_RETENTION_DAYS: i32 = 30;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineSummary {
    pub source_id: String,
    pub relative_path: String,
    pub library_name: String,
    pub sha256: String,
    pub byte_size: i64,
    pub revision: i64,
    pub retention_until_unix_ms: i64,
    pub retention_expired: bool,
}

/// List one bounded page of current quarantines without exposing host paths.
///
/// # Errors
/// Returns a database diagnostic when lifecycle evidence cannot be loaded.
pub async fn list(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<crate::api_models::Page<QuarantineSummary>, String> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT source.public_id::text,quarantine.original_relative_path,root.display_name,
         quarantine.sha256,quarantine.byte_size,quarantine.revision,
         round(extract(epoch FROM quarantine.retention_until)*1000)::bigint,
         quarantine.retention_until<=now()
         FROM volund.source_quarantines quarantine JOIN volund.source_files source ON source.id=quarantine.source_file_id
         JOIN volund.library_roots root ON root.id=source.library_root_id WHERE quarantine.state='quarantined'
         ORDER BY quarantine.retention_until,quarantine.id LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list quarantines: {error}"))?;
    let total = sqlx::query_scalar(
        "SELECT count(*) FROM volund.source_quarantines WHERE state='quarantined'",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count quarantines: {error}"))?;
    Ok(crate::api_models::Page {
        items: rows
            .iter()
            .map(|row| QuarantineSummary {
                source_id: row.get(0),
                relative_path: row.get(1),
                library_name: row.get(2),
                sha256: row.get(3),
                byte_size: row.get(4),
                revision: row.get(5),
                retention_until_unix_ms: row.get(6),
                retention_expired: row.get(7),
            })
            .collect(),
        limit,
        offset,
        total,
    })
}

struct Plan {
    action: String,
    target_id: String,
    revision: i64,
}
struct Source {
    id: i64,
    public_id: String,
    root_path: PathBuf,
    relative_path: String,
    sha256: String,
    byte_size: i64,
    lifecycle_state: String,
    lifecycle_revision: i64,
    quarantine_id: Option<i64>,
    quarantine_path: Option<String>,
    quarantine_revision: Option<i64>,
    retention_expired: bool,
}

/// Apply a source lifecycle plan with verified no-overwrite filesystem operations.
///
/// # Errors
/// Returns a classified error for invalid plans, stale state, unsafe paths, changed bytes, or persistence failure.
pub async fn apply(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    plan_id: &str,
    request: &ApplyRequest,
) -> Result<ApplySummary, LifecycleError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(database("begin source lifecycle"))?;
    let row = sqlx::query(
        "SELECT action,target_public_id::text,expected_revision,confirmation FROM volund.lifecycle_plans
         WHERE public_id::text=$1 AND actor_user_id=$2 AND consumed_at IS NULL AND expires_at>now() FOR UPDATE")
        .bind(plan_id).bind(actor.database_user_id()).fetch_optional(&mut *tx).await
        .map_err(database("lock source lifecycle plan"))?.ok_or(LifecycleError::NotFound)?;
    if request.confirmation != row.get::<String, _>(3) {
        return Err(LifecycleError::BadRequest(
            "exact lifecycle confirmation is required".to_owned(),
        ));
    }
    let plan = Plan {
        action: row.get(0),
        target_id: row.get(1),
        revision: row.get(2),
    };
    authorize(actor, &plan.action)?;
    let source = load_source(&mut tx, &plan.target_id).await?;
    crate::source_cleanup::finish_locked(&mut tx, source.id)
        .await
        .map_err(LifecycleError::Database)?;
    let mut link = crate::source_link::StagedLink::default();
    let revision = match plan.action.as_str() {
        "source.quarantine" => quarantine(&mut tx, actor, &plan, &source, &mut link).await?,
        "source.recover" => recover(&mut tx, actor, &plan, &source, &mut link).await?,
        "source.purge" => purge(&mut tx, actor, &plan, &source).await?,
        _ => {
            return Err(LifecycleError::BadRequest(
                "plan is not a source lifecycle action".to_owned(),
            ));
        }
    };
    sqlx::query("UPDATE volund.lifecycle_plans SET consumed_at=now() WHERE public_id::text=$1")
        .bind(plan_id)
        .execute(&mut *tx)
        .await
        .map_err(database("consume source lifecycle plan"))?;
    catalog_audit::record(
        &mut tx,
        actor,
        &plan.action,
        "source-file",
        &plan.target_id,
        json!({"revision":revision,"planId":plan_id}),
    )
    .await
    .map_err(|error| LifecycleError::Database(format!("cannot audit source lifecycle: {error}")))?;
    // A failed COMMIT response is ambiguous. Preserve both links; the durable
    // cleanup intent exists exactly when the catalog transition committed.
    link.preserve();
    tx.commit()
        .await
        .map_err(database("commit source lifecycle"))?;
    crate::source_cleanup::finish(pool, source.id)
        .await
        .map_err(|error| {
            LifecycleError::Database(format!(
                "lifecycle committed; file cleanup pending retry: {error}"
            ))
        })?;
    Ok(ApplySummary {
        action: plan.action,
        target_id: plan.target_id,
        outcome: "applied",
        revision,
    })
}

async fn load_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source_id: &str,
) -> Result<Source, LifecycleError> {
    let row=sqlx::query(
        "SELECT source.id,source.public_id::text,root.filesystem_path,source.relative_path,
         content.sha256,content.byte_size,source.lifecycle_state,source.lifecycle_revision,
         quarantine.id,quarantine.quarantine_relative_path,quarantine.revision,
         COALESCE(quarantine.retention_until<=now(),false)
         FROM volund.source_files source JOIN volund.library_roots root ON root.id=source.library_root_id
         JOIN volund.content_objects content ON content.id=source.content_object_id
         LEFT JOIN volund.source_quarantines quarantine ON quarantine.source_file_id=source.id AND quarantine.state='quarantined'
         WHERE source.public_id::text=$1 FOR UPDATE OF source")
        .bind(source_id).fetch_optional(&mut **tx).await.map_err(database("load source lifecycle state"))?
        .ok_or(LifecycleError::NotFound)?;
    Ok(Source {
        id: row.get(0),
        public_id: row.get(1),
        root_path: row.get::<String, _>(2).into(),
        relative_path: row.get(3),
        sha256: row.get(4),
        byte_size: row.get(5),
        lifecycle_state: row.get(6),
        lifecycle_revision: row.get(7),
        quarantine_id: row.get(8),
        quarantine_path: row.get(9),
        quarantine_revision: row.get(10),
        retention_expired: row.get(11),
    })
}

async fn quarantine(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    plan: &Plan,
    source: &Source,
    link: &mut crate::source_link::StagedLink,
) -> Result<i64, LifecycleError> {
    if source.lifecycle_state != "available" || source.lifecycle_revision != plan.revision {
        return Err(stale());
    }
    let (root, original) = available_path(source).await?;
    verify_file(&original, source).await?;
    let directory = root.join(QUARANTINE_DIRECTORY);
    fs::create_dir_all(&directory)
        .await
        .map_err(storage("create quarantine directory"))?;
    let directory = fs::canonicalize(&directory)
        .await
        .map_err(storage("resolve quarantine directory"))?;
    if !directory.starts_with(&root) {
        return Err(LifecycleError::Conflict(
            "quarantine directory escapes the library root".to_owned(),
        ));
    }
    crate::source_link::sync_parent(&directory)
        .await
        .map_err(LifecycleError::Database)?;
    let quarantine_name = format!("{}-{}", source.public_id, source.lifecycle_revision);
    let relative = format!("{QUARANTINE_DIRECTORY}/{quarantine_name}");
    let target = directory.join(quarantine_name);
    let id:i64=sqlx::query_scalar(
        "INSERT INTO volund.source_quarantines (source_file_id,original_relative_path,quarantine_relative_path,sha256,byte_size,state,retention_until,quarantined_by_user_id)
         VALUES ($1,$2,$3,$4,$5,'prepared',now()+make_interval(days=>$6),$7) RETURNING id")
        .bind(source.id).bind(&source.relative_path).bind(&relative).bind(&source.sha256).bind(source.byte_size)
        .bind(retention_days()?).bind(actor.database_user_id()).fetch_one(&mut **tx).await.map_err(database("prepare source quarantine"))?;
    link.create(&original, &target)
        .await
        .map_err(LifecycleError::Database)?;
    verify_file(&target, source).await?;
    persist_quarantine(tx, source.id, id).await?;
    crate::source_cleanup::enqueue(
        tx,
        source.id,
        &source.relative_path,
        Some(&relative),
        &source.sha256,
        source.byte_size,
    )
    .await
    .map_err(LifecycleError::Database)?;
    Ok(source.lifecycle_revision + 1)
}

async fn persist_quarantine(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source_id: i64,
    quarantine_id: i64,
) -> Result<(), LifecycleError> {
    sqlx::query(
        "UPDATE volund.source_quarantines SET state='quarantined',updated_at=now() WHERE id=$1",
    )
    .bind(quarantine_id)
    .execute(&mut **tx)
    .await
    .map_err(database("mark source quarantined"))?;
    sqlx::query("UPDATE volund.source_files SET lifecycle_state='quarantined',lifecycle_revision=lifecycle_revision+1,missing_at=COALESCE(missing_at,now()) WHERE id=$1")
        .bind(source_id).execute(&mut **tx).await.map_err(database("update quarantined source"))?;
    Ok(())
}

async fn recover(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    plan: &Plan,
    source: &Source,
    link: &mut crate::source_link::StagedLink,
) -> Result<i64, LifecycleError> {
    if source.lifecycle_state != "quarantined" || source.quarantine_revision != Some(plan.revision)
    {
        return Err(stale());
    }
    let id = source.quarantine_id.ok_or(LifecycleError::NotFound)?;
    let (root, quarantined) = quarantine_path(source).await?;
    verify_file(&quarantined, source).await?;
    let destination = root.join(&source.relative_path);
    let parent = destination
        .parent()
        .ok_or_else(|| LifecycleError::Conflict("invalid recovery path".to_owned()))?;
    let parent = fs::canonicalize(parent)
        .await
        .map_err(storage("resolve recovery directory"))?;
    if !parent.starts_with(&root) {
        return Err(LifecycleError::Conflict(
            "recovery destination is unavailable or already exists".to_owned(),
        ));
    }
    link.create(&quarantined, &destination)
        .await
        .map_err(LifecycleError::Conflict)?;
    verify_file(&destination, source).await?;
    crate::source_cleanup::enqueue(
        tx,
        source.id,
        source
            .quarantine_path
            .as_deref()
            .ok_or(LifecycleError::NotFound)?,
        Some(&source.relative_path),
        &source.sha256,
        source.byte_size,
    )
    .await
    .map_err(LifecycleError::Database)?;
    sqlx::query("UPDATE volund.source_quarantines SET state='recovered',revision=revision+1,recovered_by_user_id=$2,updated_at=now() WHERE id=$1")
            .bind(id).bind(actor.database_user_id()).execute(&mut **tx).await.map_err(database("mark quarantine recovered"))?;
    sqlx::query_scalar("UPDATE volund.source_files SET lifecycle_state='available',lifecycle_revision=lifecycle_revision+1,missing_at=NULL WHERE id=$1 RETURNING lifecycle_revision")
            .bind(source.id).fetch_one(&mut **tx).await.map_err(database("restore source lifecycle"))
}

async fn purge(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    plan: &Plan,
    source: &Source,
) -> Result<i64, LifecycleError> {
    if source.lifecycle_state != "quarantined"
        || source.quarantine_revision != Some(plan.revision)
        || !source.retention_expired
    {
        return Err(stale());
    }
    let protected:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM volund.model_source_files WHERE source_file_id=$1)+(SELECT count(*) FROM volund.models WHERE thumbnail_source_file_id=$1)+(SELECT count(*) FROM volund.file_dependencies WHERE source_file_id=$1 OR resolved_source_file_id=$1)")
        .bind(source.id).fetch_one(&mut **tx).await.map_err(database("check purge references"))?;
    if protected != 0 {
        return Err(LifecycleError::Conflict(
            "source still has protected live references".to_owned(),
        ));
    }
    let id = source.quarantine_id.ok_or(LifecycleError::NotFound)?;
    let (_, path) = quarantine_path(source).await?;
    verify_file(&path, source).await?;
    crate::source_cleanup::enqueue(
        tx,
        source.id,
        source
            .quarantine_path
            .as_deref()
            .ok_or(LifecycleError::NotFound)?,
        None,
        &source.sha256,
        source.byte_size,
    )
    .await
    .map_err(LifecycleError::Database)?;
    sqlx::query("UPDATE volund.source_quarantines SET state='purged',revision=revision+1,purged_by_user_id=$2,updated_at=now() WHERE id=$1")
        .bind(id).bind(actor.database_user_id()).execute(&mut **tx).await.map_err(database("record source purge"))?;
    sqlx::query_scalar("UPDATE volund.source_files SET lifecycle_state='purged',lifecycle_revision=lifecycle_revision+1 WHERE id=$1 RETURNING lifecycle_revision")
        .bind(source.id).fetch_one(&mut **tx).await.map_err(database("mark source purged"))
}

async fn available_path(source: &Source) -> Result<(PathBuf, PathBuf), LifecycleError> {
    let root = canonical_root(&source.root_path).await?;
    let path = fs::canonicalize(root.join(&source.relative_path))
        .await
        .map_err(storage("resolve source original"))?;
    if !path.starts_with(&root) {
        return Err(LifecycleError::Conflict(
            "source path escapes its library root".to_owned(),
        ));
    }
    Ok((root, path))
}
async fn quarantine_path(source: &Source) -> Result<(PathBuf, PathBuf), LifecycleError> {
    let root = canonical_root(&source.root_path).await?;
    let relative = source
        .quarantine_path
        .as_deref()
        .ok_or(LifecycleError::NotFound)?;
    let path = fs::canonicalize(root.join(relative))
        .await
        .map_err(storage("resolve quarantined original"))?;
    if !path.starts_with(root.join(QUARANTINE_DIRECTORY)) {
        return Err(LifecycleError::Conflict(
            "quarantine path escaped its managed directory".to_owned(),
        ));
    }
    Ok((root, path))
}
async fn canonical_root(path: &Path) -> Result<PathBuf, LifecycleError> {
    fs::canonicalize(path)
        .await
        .map_err(storage("resolve library root"))
}
async fn verify_file(path: &Path, source: &Source) -> Result<(), LifecycleError> {
    let metadata = fs::metadata(path)
        .await
        .map_err(storage("inspect lifecycle file"))?;
    if !metadata.is_file() || i64::try_from(metadata.len()).ok() != Some(source.byte_size) {
        return Err(LifecycleError::Conflict(
            "source size changed after indexing".to_owned(),
        ));
    }
    let owned = path.to_owned();
    let digest = tokio::task::spawn_blocking(move || sha256_file(&owned))
        .await
        .map_err(|error| {
            LifecycleError::Database(format!("cannot join hash verification: {error}"))
        })?
        .map_err(LifecycleError::Conflict)?;
    if digest.as_str() != source.sha256 {
        return Err(LifecycleError::Conflict(
            "source hash changed after indexing".to_owned(),
        ));
    }
    Ok(())
}
fn authorize(actor: &AuthenticatedSession, action: &str) -> Result<(), LifecycleError> {
    let allowed = if action == "source.purge" {
        actor.role == "owner"
    } else {
        matches!(actor.role.as_str(), "owner" | "administrator")
    };
    if allowed {
        Ok(())
    } else {
        Err(LifecycleError::Forbidden)
    }
}
fn retention_days() -> Result<i32, LifecycleError> {
    match std::env::var("VOLUND_QUARANTINE_RETENTION_DAYS") {
        Ok(value) => value
            .parse::<i32>()
            .ok()
            .filter(|days| (1..=3650).contains(days))
            .ok_or_else(|| {
                LifecycleError::BadRequest(
                    "VOLUND_QUARANTINE_RETENTION_DAYS must be between 1 and 3650".to_owned(),
                )
            }),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_RETENTION_DAYS),
        Err(error) => Err(LifecycleError::Database(format!(
            "cannot read quarantine retention: {error}"
        ))),
    }
}
fn stale() -> LifecycleError {
    LifecycleError::Conflict("source changed after impact preview".to_owned())
}
fn database(context: &'static str) -> impl Fn(sqlx::Error) -> LifecycleError {
    move |error| LifecycleError::Database(format!("cannot {context}: {error}"))
}
fn storage(context: &'static str) -> impl Fn(std::io::Error) -> LifecycleError {
    move |error| LifecycleError::Database(format!("cannot {context}: {error}"))
}
