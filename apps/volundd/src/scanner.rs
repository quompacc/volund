use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sqlx::{PgPool, Row};
use volund_core::{CadFormat, ContentHash, normalize_library_path};

use crate::file_hash::sha256_file;
use crate::scan_control;

const SCAN_LOCK_NAMESPACE: i64 = 6_219_273_558_347_087_872;
const QUARANTINE_DIRECTORY: &str = ".volund-quarantine";

#[derive(Debug, Eq, PartialEq)]
pub struct ScanReport {
    pub discovered_files: i64,
    pub hashed_files: i64,
    pub missing_files: i64,
}

#[derive(Debug)]
struct LibraryRoot {
    id: i64,
    path: PathBuf,
}

#[derive(Debug)]
struct ExistingFile {
    content_id: i64,
    byte_size: i64,
    modified_micros: i64,
}

#[derive(Debug)]
struct DiscoveredFile {
    relative_path: String,
    format: Option<CadFormat>,
    byte_size: i64,
    modified_micros: i64,
    device: Option<i64>,
    inode: Option<i64>,
    content: ContentSource,
}

#[derive(Debug)]
enum ContentSource {
    Existing(i64),
    Hashed(ContentHash),
}

/// Register an existing Unix directory as a read-only library root.
///
/// Repeating the same key and path updates only its display name. A key can
/// never silently be redirected to a different directory.
///
/// # Errors
///
/// Returns an error when the directory cannot be resolved/inspected, is not a
/// valid absolute Unix path, conflicts with an existing root, or cannot be
/// persisted.
pub async fn register_root(
    pool: &PgPool,
    key: &str,
    display_name: &str,
    path: &Path,
) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve library root {}: {error}", path.display()))?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        format!(
            "cannot inspect library root {}: {error}",
            canonical.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "library root is not a directory: {}",
            canonical.display()
        ));
    }
    let stored_path = canonical
        .to_str()
        .ok_or_else(|| "library root path must be valid UTF-8".to_owned())?;
    if !stored_path.starts_with('/') {
        return Err("library roots must use an absolute Unix path".to_owned());
    }

    let inserted = sqlx::query_scalar::<_, String>(
        "INSERT INTO volund.library_roots (root_key, display_name, filesystem_path) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (root_key) DO UPDATE SET display_name = EXCLUDED.display_name \
         WHERE volund.library_roots.filesystem_path = EXCLUDED.filesystem_path \
         RETURNING filesystem_path",
    )
    .bind(key)
    .bind(display_name)
    .bind(stored_path)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot register library root: {error}"))?;
    if inserted.is_none() {
        return Err(format!(
            "library key '{key}' is already assigned to another filesystem path"
        ));
    }
    Ok(canonical)
}

/// Scan one registered library root and atomically persist the resulting view.
///
/// # Errors
///
/// Returns an error when the root is unknown/unreadable, a CAD file changes
/// while being hashed, or `PostgreSQL` cannot record the scan. Failed runs are
/// recorded without partially updating source-file state.
pub async fn scan_root(pool: &PgPool, root_key: &str, full: bool) -> Result<ScanReport, String> {
    let root = load_root(pool, root_key).await?;
    scan_with_lock(pool, &root, root_key, full, None).await
}

/// Execute a scan run that a durable queue worker has already claimed.
///
/// # Errors
///
/// Returns an error when the claimed run does not belong to the requested
/// library, another scanner owns its advisory lock, or scanning fails.
pub async fn scan_queued(
    pool: &PgPool,
    scan_id: i64,
    root_key: &str,
    full: bool,
) -> Result<ScanReport, String> {
    let root = load_root(pool, root_key).await?;
    let claim = sqlx::query(
        "SELECT status, cancellation_requested_at IS NOT NULL FROM volund.scan_runs \
         WHERE id = $1 AND library_root_id = $2",
    )
    .bind(scan_id)
    .bind(root.id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot validate claimed scan: {error}"))?
    .ok_or_else(|| "scan queue claim is stale or belongs to another library".to_owned())?;
    if claim.get::<String, _>(0) != "running" {
        return Err("scan queue claim is stale or belongs to another library".to_owned());
    }
    if claim.get::<bool, _>(1) {
        scan_control::mark_cancelled(pool, scan_id).await;
        return Err("scan cancellation requested".to_owned());
    }
    scan_with_lock(pool, &root, root_key, full, Some(scan_id)).await
}

async fn scan_with_lock(
    pool: &PgPool,
    root: &LibraryRoot,
    root_key: &str,
    full: bool,
    claimed_scan_id: Option<i64>,
) -> Result<ScanReport, String> {
    let mut lock_connection = pool
        .acquire()
        .await
        .map_err(|error| format!("cannot acquire scan lock connection: {error}"))?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1 + $2)")
        .bind(SCAN_LOCK_NAMESPACE)
        .bind(root.id)
        .fetch_one(&mut *lock_connection)
        .await
        .map_err(|error| format!("cannot acquire scan lock: {error}"))?;
    if !locked {
        let message = format!("a scan is already running for library root '{root_key}'");
        if let Some(scan_id) = claimed_scan_id {
            mark_failed(pool, scan_id, &message).await;
        }
        return Err(message);
    }
    let result = scan_locked(pool, root, full, claimed_scan_id).await;
    let _ = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1 + $2)")
        .bind(SCAN_LOCK_NAMESPACE)
        .bind(root.id)
        .fetch_one(&mut *lock_connection)
        .await;
    result
}

async fn scan_locked(
    pool: &PgPool,
    root: &LibraryRoot,
    full: bool,
    claimed_scan_id: Option<i64>,
) -> Result<ScanReport, String> {
    let scan_id = if let Some(scan_id) = claimed_scan_id {
        scan_id
    } else {
        sqlx::query_scalar(
            "INSERT INTO volund.scan_runs (library_root_id, status, full_scan) \
             VALUES ($1, 'running', $2) RETURNING id",
        )
        .bind(root.id)
        .bind(full)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("cannot start scan: {error}"))?
    };

    let result = prepare_scan(pool, root, full, claimed_scan_id).await;
    let discovered = match result {
        Ok(discovered) => discovered,
        Err(message) => {
            if message == "scan cancellation requested" {
                scan_control::mark_cancelled(pool, scan_id).await;
            } else {
                mark_failed(pool, scan_id, &message).await;
            }
            return Err(message);
        }
    };
    match persist_scan(pool, root, scan_id, discovered).await {
        Ok(report) => Ok(report),
        Err(message) => {
            mark_failed(pool, scan_id, &message).await;
            Err(message)
        }
    }
}

async fn load_root(pool: &PgPool, root_key: &str) -> Result<LibraryRoot, String> {
    let row = sqlx::query(
        "SELECT id, filesystem_path, enabled FROM volund.library_roots WHERE root_key = $1",
    )
    .bind(root_key)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load library root: {error}"))?
    .ok_or_else(|| format!("unknown library root: {root_key}"))?;
    if !row.get::<bool, _>(2) {
        return Err(format!("library root is disabled: {root_key}"));
    }
    Ok(LibraryRoot {
        id: row.get(0),
        path: PathBuf::from(row.get::<String, _>(1)),
    })
}

async fn prepare_scan(
    pool: &PgPool,
    root: &LibraryRoot,
    full: bool,
    claimed_scan_id: Option<i64>,
) -> Result<Vec<DiscoveredFile>, String> {
    let existing = load_existing(pool, root.id).await?;
    let candidates = discover_files(&root.path)?;
    let candidate_count = candidates.len();
    let mut discovered = Vec::with_capacity(candidates.len());
    if let Some(scan_id) = claimed_scan_id {
        scan_control::update_progress(pool, scan_id, candidate_count, 0).await?;
    }
    let mut hashed = 0_usize;
    for (path, relative_path, format) in candidates {
        if let Some(scan_id) = claimed_scan_id {
            scan_control::ensure_not_cancelled(pool, scan_id).await?;
        }
        let before = fs::metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        let byte_size = i64::try_from(before.len())
            .map_err(|_| format!("file is too large to index: {}", path.display()))?;
        let modified_micros = filesystem_modified_micros(&before, &path)?;
        let unchanged = existing.get(&relative_path).filter(|item| {
            !full && item.byte_size == byte_size && item.modified_micros == modified_micros
        });
        let content = if let Some(item) = unchanged {
            ContentSource::Existing(item.content_id)
        } else {
            let hash = sha256_file(&path)?;
            hashed += 1;
            let after = fs::metadata(&path)
                .map_err(|error| format!("cannot re-inspect {}: {error}", path.display()))?;
            if before.len() != after.len()
                || modified_micros != filesystem_modified_micros(&after, &path)?
            {
                return Err(format!(
                    "file changed while it was being hashed: {}",
                    path.display()
                ));
            }
            ContentSource::Hashed(hash)
        };
        let (device, inode) = filesystem_identity(&before);
        discovered.push(DiscoveredFile {
            relative_path,
            format,
            byte_size,
            modified_micros,
            device,
            inode,
            content,
        });
        if let Some(scan_id) = claimed_scan_id {
            scan_control::update_progress(pool, scan_id, candidate_count, hashed).await?;
        }
    }
    Ok(discovered)
}

async fn load_existing(
    pool: &PgPool,
    root_id: i64,
) -> Result<HashMap<String, ExistingFile>, String> {
    let rows = sqlx::query(
        "SELECT source.relative_path, source.content_object_id, content.byte_size, \
         round(extract(epoch FROM source.filesystem_modified_at) * 1000000)::bigint \
         FROM volund.source_files source \
         JOIN volund.content_objects content ON content.id = source.content_object_id \
         WHERE source.library_root_id = $1",
    )
    .bind(root_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot load existing library state: {error}"))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.get(0),
                ExistingFile {
                    content_id: row.get(1),
                    byte_size: row.get(2),
                    modified_micros: row.get(3),
                },
            )
        })
        .collect())
}

fn discover_files(root: &Path) -> Result<Vec<(PathBuf, String, Option<CadFormat>)>, String> {
    let root_metadata = fs::metadata(root)
        .map_err(|error| format!("cannot inspect library root {}: {error}", root.display()))?;
    if !root_metadata.is_dir() {
        return Err(format!(
            "library root is not a directory: {}",
            root.display()
        ));
    }
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("cannot read directory {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("cannot read entry in {}: {error}", directory.display())
            })?;
            if directory == root && entry.file_name() == QUARANTINE_DIRECTORY {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let entry_path = entry.path();
            let format = entry_path
                .extension()
                .and_then(|value| value.to_str())
                .and_then(CadFormat::from_extension);
            let relative = entry_path
                .strip_prefix(root)
                .map_err(|_| {
                    format!(
                        "discovered path escaped library root: {}",
                        entry.path().display()
                    )
                })?
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| "library paths must be valid UTF-8".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            files.push((entry_path, normalize_library_path(&relative)?, format));
        }
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(files)
}

async fn persist_scan(
    pool: &PgPool,
    root: &LibraryRoot,
    scan_id: i64,
    discovered: Vec<DiscoveredFile>,
) -> Result<ScanReport, String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin scan transaction: {error}"))?;
    if scan_control::cancel_before_persist(&mut transaction, scan_id).await? {
        transaction
            .commit()
            .await
            .map_err(|error| format!("cannot commit scan cancellation: {error}"))?;
        return Err("scan cancellation requested".to_owned());
    }
    let mut hashed_files = 0_i64;
    for file in &discovered {
        let content_id = match &file.content {
            ContentSource::Existing(id) => *id,
            ContentSource::Hashed(hash) => {
                hashed_files += 1;
                sqlx::query_scalar(
                    "INSERT INTO volund.content_objects \
                     (sha256, byte_size, detected_format) VALUES ($1, $2, $3) \
                     ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 \
                     RETURNING id",
                )
                .bind(hash.as_str())
                .bind(file.byte_size)
                .bind(file.format.map(CadFormat::as_str))
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| format!("cannot persist content hash: {error}"))?
            }
        };
        sqlx::query(
            "INSERT INTO volund.source_files \
             (library_root_id, content_object_id, relative_path, filesystem_modified_at, \
              filesystem_device, filesystem_inode, last_seen_scan_id) \
             VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000000.0), $5, $6, $7) \
             ON CONFLICT (library_root_id, relative_path) DO UPDATE SET \
              content_object_id = EXCLUDED.content_object_id, \
              filesystem_modified_at = EXCLUDED.filesystem_modified_at, \
              filesystem_device = EXCLUDED.filesystem_device, \
              filesystem_inode = EXCLUDED.filesystem_inode, \
              last_seen_scan_id = EXCLUDED.last_seen_scan_id, missing_at = NULL",
        )
        .bind(root.id)
        .bind(content_id)
        .bind(&file.relative_path)
        .bind(file.modified_micros)
        .bind(file.device)
        .bind(file.inode)
        .bind(scan_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("cannot persist source path {}: {error}", file.relative_path))?;
    }
    let missing_files = sqlx::query(
        "UPDATE volund.source_files SET missing_at = now() \
         WHERE library_root_id = $1 AND last_seen_scan_id <> $2 AND missing_at IS NULL",
    )
    .bind(root.id)
    .bind(scan_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("cannot mark missing source paths: {error}"))?
    .rows_affected()
    .try_into()
    .map_err(|_| "missing file count overflow".to_owned())?;
    let discovered_files =
        i64::try_from(discovered.len()).map_err(|_| "discovered file count overflow".to_owned())?;
    sqlx::query(
        "UPDATE volund.scan_runs SET status = 'completed', finished_at = now(), \
         discovered_files = $2, hashed_files = $3, missing_files = $4 WHERE id = $1",
    )
    .bind(scan_id)
    .bind(discovered_files)
    .bind(hashed_files)
    .bind(missing_files)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("cannot complete scan run: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("cannot commit scan: {error}"))?;
    Ok(ScanReport {
        discovered_files,
        hashed_files,
        missing_files,
    })
}

async fn mark_failed(pool: &PgPool, scan_id: i64, message: &str) {
    let _ = sqlx::query(
        "UPDATE volund.scan_runs SET status = 'failed', finished_at = now(), error_message = $2 \
         WHERE id = $1 AND status = 'running'",
    )
    .bind(scan_id)
    .bind(message)
    .execute(pool)
    .await;
}

fn filesystem_modified_micros(metadata: &Metadata, path: &Path) -> Result<i64, String> {
    let duration = metadata
        .modified()
        .map_err(|error| {
            format!(
                "cannot read modification time of {}: {error}",
                path.display()
            )
        })?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| format!("modification time predates Unix epoch: {}", path.display()))?;
    i64::try_from(duration.as_micros())
        .map_err(|_| format!("modification time is out of range: {}", path.display()))
}

#[cfg(unix)]
fn filesystem_identity(metadata: &Metadata) -> (Option<i64>, Option<i64>) {
    use std::os::unix::fs::MetadataExt;
    (
        i64::try_from(metadata.dev()).ok(),
        i64::try_from(metadata.ino()).ok(),
    )
}

#[cfg(not(unix))]
fn filesystem_identity(_metadata: &Metadata) -> (Option<i64>, Option<i64>) {
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("volund-discovery-{unique}"))
    }

    #[test]
    fn discovery_tracks_all_regular_files_and_classifies_cad() {
        let root = temporary_root();
        fs::create_dir_all(root.join("nested")).expect("create fixture");
        fs::write(root.join("z.STP"), b"z").expect("write fixture");
        fs::write(root.join("nested/a.glb"), b"a").expect("write fixture");
        fs::write(root.join("notes.txt"), b"notes").expect("write fixture");
        let files = discover_files(&root).expect("discover library files");
        assert_eq!(
            files.iter().map(|item| item.1.as_str()).collect::<Vec<_>>(),
            ["nested/a.glb", "notes.txt", "z.STP"]
        );
        assert_eq!(files[0].2, Some(CadFormat::Glb));
        assert_eq!(files[1].2, None);
        assert_eq!(files[2].2, Some(CadFormat::Step));
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
