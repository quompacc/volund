use std::path::{Path, PathBuf};

use sqlx::{PgPool, Row};
use tokio::fs;
use volund_core::normalize_library_path;

const MOVE_LOCK_NAMESPACE: i64 = 6_219_273_558_347_087_872;

#[derive(Debug, Eq, PartialEq)]
pub struct MoveResult {
    pub id: String,
    pub previous_path: String,
    pub path: String,
}

#[derive(Debug)]
pub enum MoveError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Busy(String),
    Storage(String),
    Database(String),
}

struct SourceRecord {
    id: i64,
    public_id: String,
    root_id: i64,
    root_path: PathBuf,
    relative_path: String,
    missing: bool,
    hash: String,
    size: i64,
}

/// Move an indexed file inside its current library while preserving its public ID.
///
/// The target directory must already exist. A hard-link-first move prevents an
/// existing destination from being overwritten. Original paths survive until
/// catalog commit; durable cleanup retires them afterward. Ambiguous commit
/// outcomes retain both paths rather than attempting a destructive rollback.
///
/// # Errors
///
/// Returns a classified error for invalid paths, unknown or missing files,
/// destination conflicts, concurrent scans, storage failures, or database
/// failures.
pub async fn move_file(
    pool: &PgPool,
    file_id: &str,
    destination_directory: &str,
) -> Result<MoveResult, MoveError> {
    let destination_directory = normalize_directory(destination_directory)?;
    let mut transaction = pool.begin().await.map_err(|error| database_error(&error))?;
    let source = load_source(&mut transaction, file_id).await?;
    acquire_move_lock(&mut transaction, &source).await?;
    if source.missing {
        return Err(MoveError::NotFound(format!(
            "source file is missing: {file_id}"
        )));
    }
    let file_name = Path::new(&source.relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| MoveError::BadRequest("source filename is invalid".to_owned()))?;
    let new_relative_path = join_relative_path(&destination_directory, file_name);
    if new_relative_path == source.relative_path {
        return Err(MoveError::BadRequest(
            "source file is already in the selected directory".to_owned(),
        ));
    }
    let (source_path, target_path) =
        resolve_paths(&source, &destination_directory, file_name).await?;
    crate::source_cleanup::finish_locked(&mut transaction, source.id)
        .await
        .map_err(MoveError::Storage)?;
    let hash_path = source_path.clone();
    let hash = tokio::task::spawn_blocking(move || crate::file_hash::sha256_file(&hash_path))
        .await
        .map_err(|e| MoveError::Storage(e.to_string()))?
        .map_err(MoveError::Storage)?;
    if hash.as_str() != source.hash {
        return Err(MoveError::Conflict(
            "source contents changed; scan again before moving".to_owned(),
        ));
    }
    let mut link = crate::source_link::StagedLink::default();
    link.create(&source_path, &target_path)
        .await
        .map_err(MoveError::Conflict)?;
    crate::source_cleanup::enqueue(
        &mut transaction,
        source.id,
        &source.relative_path,
        Some(&new_relative_path),
        &source.hash,
        source.size,
    )
    .await
    .map_err(MoveError::Database)?;
    persist_move(&mut transaction, &source, &new_relative_path).await?;
    // Once COMMIT begins its outcome can be ambiguous; never guess by unlinking.
    link.preserve();
    transaction
        .commit()
        .await
        .map_err(|error| database_error(&error))?;
    crate::source_cleanup::finish(pool, source.id)
        .await
        .map_err(|error| {
            MoveError::Storage(format!(
                "move committed; source cleanup pending retry: {error}"
            ))
        })?;
    Ok(MoveResult {
        id: source.public_id,
        previous_path: source.relative_path,
        path: new_relative_path,
    })
}

fn normalize_directory(directory: &str) -> Result<String, MoveError> {
    let trimmed = directory.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let normalized = normalize_library_path(trimmed)
        .map_err(|message| MoveError::BadRequest(message.to_owned()))?;
    if Path::new(&normalized).starts_with(".volund-quarantine") {
        return Err(MoveError::BadRequest(
            "the internal quarantine directory is not a move destination".to_owned(),
        ));
    }
    Ok(normalized)
}

fn join_relative_path(directory: &str, file_name: &str) -> String {
    if directory.is_empty() {
        file_name.to_owned()
    } else {
        format!("{directory}/{file_name}")
    }
}

async fn load_source(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    file_id: &str,
) -> Result<SourceRecord, MoveError> {
    let row = sqlx::query(
        "SELECT source.id, source.public_id::text, root.id, root.filesystem_path, \
         source.relative_path, source.missing_at IS NOT NULL, content.sha256, content.byte_size \
         FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id = source.library_root_id \
         JOIN volund.content_objects content ON content.id = source.content_object_id \
         WHERE source.public_id::text = $1 FOR UPDATE OF source",
    )
    .bind(file_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| database_error(&error))?
    .ok_or_else(|| MoveError::NotFound(format!("unknown source file: {file_id}")))?;
    Ok(SourceRecord {
        id: row.get(0),
        public_id: row.get(1),
        root_id: row.get(2),
        root_path: PathBuf::from(row.get::<String, _>(3)),
        relative_path: row.get(4),
        missing: row.get(5),
        hash: row.get(6),
        size: row.get(7),
    })
}

async fn acquire_move_lock(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &SourceRecord,
) -> Result<(), MoveError> {
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1 + $2)")
        .bind(MOVE_LOCK_NAMESPACE)
        .bind(source.root_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| database_error(&error))?;
    if locked {
        Ok(())
    } else {
        Err(MoveError::Busy(
            "library is currently being scanned or modified".to_owned(),
        ))
    }
}

async fn resolve_paths(
    source: &SourceRecord,
    directory: &str,
    file_name: &str,
) -> Result<(PathBuf, PathBuf), MoveError> {
    let root = fs::canonicalize(&source.root_path)
        .await
        .map_err(|error| storage_error("resolve library root", &error))?;
    let source_path = fs::canonicalize(root.join(&source.relative_path))
        .await
        .map_err(|error| storage_error("resolve source file", &error))?;
    let mut checked = root.clone();
    for component in Path::new(directory).components() {
        checked.push(component);
        let metadata = fs::symlink_metadata(&checked)
            .await
            .map_err(|error| storage_error("inspect destination path", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(MoveError::BadRequest(
                "destination must contain only real directories, not symbolic links".to_owned(),
            ));
        }
    }
    let target_directory = fs::canonicalize(root.join(directory))
        .await
        .map_err(|error| storage_error("resolve destination directory", &error))?;
    if !source_path.starts_with(&root) || !target_directory.starts_with(&root) {
        return Err(MoveError::BadRequest(
            "move path escapes the library root".to_owned(),
        ));
    }
    let metadata = fs::metadata(&target_directory)
        .await
        .map_err(|error| storage_error("inspect destination directory", &error))?;
    if !metadata.is_dir() {
        return Err(MoveError::BadRequest(
            "destination is not a directory".to_owned(),
        ));
    }
    Ok((source_path, target_directory.join(file_name)))
}

async fn persist_move(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &SourceRecord,
    new_path: &str,
) -> Result<(), MoveError> {
    sqlx::query("UPDATE volund.source_files SET relative_path = $2 WHERE id = $1")
        .bind(source.id)
        .bind(new_path)
        .execute(&mut **transaction)
        .await
        .map_err(|error| database_error(&error))?;
    sqlx::query(
        "INSERT INTO volund.source_file_moves \
         (source_file_id, previous_relative_path, new_relative_path) VALUES ($1, $2, $3)",
    )
    .bind(source.id)
    .bind(&source.relative_path)
    .bind(new_path)
    .execute(&mut **transaction)
    .await
    .map_err(|error| database_error(&error))?;
    Ok(())
}

fn database_error(error: &sqlx::Error) -> MoveError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        MoveError::Conflict("destination path is already indexed".to_owned())
    } else {
        MoveError::Database(format!("cannot persist source-file move: {error}"))
    }
}

fn storage_error(action: &str, error: &std::io::Error) -> MoveError {
    MoveError::Storage(format!("cannot {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_directories_are_normalized_without_allowing_traversal() {
        assert_eq!(
            normalize_directory("/assemblies/voron/").unwrap(),
            "assemblies/voron"
        );
        assert_eq!(normalize_directory("/").unwrap(), "");
        assert!(normalize_directory("../outside").is_err());
        assert!(normalize_directory("/.volund-quarantine/").is_err());
        assert!(normalize_directory(".volund-quarantine/nested").is_err());
        assert!(normalize_directory(".volund-quarantine-other").is_ok());
    }

    #[test]
    fn target_paths_preserve_the_source_filename() {
        assert_eq!(
            join_relative_path("assemblies", "voron.step"),
            "assemblies/voron.step"
        );
        assert_eq!(join_relative_path("", "voron.step"), "voron.step");
    }
}
