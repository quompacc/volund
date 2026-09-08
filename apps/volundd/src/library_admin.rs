use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;
use crate::storage_health::{self, OperationalState, StorageObservation};

#[derive(Debug, Eq, PartialEq)]
pub enum LibraryAdminError {
    BadRequest(String),
    NotFound,
    Conflict(String),
    Storage(String),
    Database(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedLibrary {
    pub revision: i64,
    pub id: String,
    pub key: String,
    pub name: String,
    pub filesystem_path: String,
    pub read_only: bool,
    pub enabled: bool,
    pub file_count: i64,
    pub missing_file_count: i64,
    pub latest_scan_status: Option<String>,
    pub latest_scan_started_at_unix_ms: Option<i64>,
    pub updated_at_unix_ms: i64,
    pub storage: StorageObservation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct PathValidation {
    pub requested_path: String,
    pub canonical_path: String,
    pub exists: bool,
    pub directory: bool,
    pub readable: bool,
    pub writable_permission: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub id: String,
    pub status: String,
    pub full: bool,
}

/// List the host-sensitive administrative view of every library.
///
/// # Errors
///
/// Returns a database error when the library state cannot be loaded.
pub async fn list(pool: &PgPool) -> Result<Vec<ManagedLibrary>, LibraryAdminError> {
    let rows = sqlx::query(MANAGED_LIBRARY_SELECT)
        .fetch_all(pool)
        .await
        .map_err(database_error("list libraries"))?;
    Ok(rows.iter().map(managed_library).collect())
}

/// Validate and canonicalize one proposed native Debian library path.
///
/// # Errors
///
/// Returns a bad-request error for an invalid, missing, non-directory, or
/// unreadable path.
pub fn validate_path(path: &str) -> Result<PathValidation, LibraryAdminError> {
    let requested = validate_requested_path(path)?;
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        LibraryAdminError::BadRequest(format!("library path cannot be resolved: {error}"))
    })?;
    let canonical_text = canonical
        .to_str()
        .ok_or_else(|| LibraryAdminError::BadRequest("library path must be valid UTF-8".into()))?;
    if !canonical_text.starts_with('/') {
        return Err(LibraryAdminError::BadRequest(
            "library path must resolve to an absolute Unix path".into(),
        ));
    }
    let metadata = fs::metadata(&canonical).map_err(|error| {
        LibraryAdminError::BadRequest(format!("library path cannot be inspected: {error}"))
    })?;
    if !metadata.is_dir() {
        return Err(LibraryAdminError::BadRequest(
            "library path is not a directory".into(),
        ));
    }
    fs::read_dir(&canonical).map_err(|error| {
        LibraryAdminError::BadRequest(format!("library directory is not readable: {error}"))
    })?;
    // Resolving the directory itself needs no search permission on it. Resolving
    // its '.' entry does, even when it is empty; listing names alone is insufficient.
    fs::metadata(canonical.join(".")).map_err(|error| {
        LibraryAdminError::BadRequest(format!("library directory is not searchable: {error}"))
    })?;
    Ok(PathValidation {
        requested_path: path.to_owned(),
        canonical_path: canonical_text.to_owned(),
        exists: true,
        directory: true,
        readable: true,
        writable_permission: has_write_permission(&metadata),
    })
}

/// Register a validated library and audit the actor in one database transaction.
///
/// # Errors
///
/// Returns validation, uniqueness-conflict, filesystem, or database errors.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    key: &str,
    name: &str,
    path: &str,
    confirmation: &str,
) -> Result<ManagedLibrary, LibraryAdminError> {
    validate_key(key)?;
    if validate_confirmation(&format!("ADD LIBRARY {key}"), confirmation).is_err() {
        crate::security_audit::denied(
            pool,
            actor,
            "library.create",
            "library",
            "confirmation_mismatch",
        )
        .await;
        return Err(LibraryAdminError::BadRequest(
            "exact library action confirmation is required".to_owned(),
        ));
    }
    let name = validate_name(name)?;
    let validation = validate_path(path)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin library creation"))?;
    let public_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO volund.library_roots \
         (root_key, display_name, filesystem_path, updated_by_user_id) \
         VALUES ($1, $2, $3, $4) RETURNING public_id::text",
    )
    .bind(key)
    .bind(name)
    .bind(&validation.canonical_path)
    .bind(actor.database_user_id())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| conflict_or_database(&error, "create library"))?;
    audit(
        &mut transaction,
        actor,
        "library.create",
        &public_id,
        json!({"key": key, "path": validation.canonical_path}),
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit library creation"))?;
    load(pool, key).await
}

/// Update safe metadata or activation state without changing stable identity.
///
/// # Errors
///
/// Returns validation, revision-conflict, not-found, or database errors.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    key: &str,
    expected_revision: i64,
    name: Option<&str>,
    enabled: Option<bool>,
    confirmation: &str,
) -> Result<ManagedLibrary, LibraryAdminError> {
    if expected_revision < 1 {
        return Err(LibraryAdminError::BadRequest(
            "expected revision must be positive".into(),
        ));
    }
    if validate_confirmation(&format!("UPDATE LIBRARY {key}"), confirmation).is_err() {
        crate::security_audit::denied(
            pool,
            actor,
            "library.update",
            "library",
            "confirmation_mismatch",
        )
        .await;
        return Err(LibraryAdminError::BadRequest(
            "exact library action confirmation is required".to_owned(),
        ));
    }
    if name.is_none() && enabled.is_none() {
        return Err(LibraryAdminError::BadRequest(
            "name or enabled must be provided".into(),
        ));
    }
    let name = name.map(validate_name).transpose()?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin library update"))?;
    let public_id = sqlx::query_scalar::<_, String>(
        "UPDATE volund.library_roots SET \
         display_name = COALESCE($2, display_name), enabled = COALESCE($3, enabled), \
         updated_at = now(), updated_by_user_id = $4, revision = revision + 1 \
         WHERE root_key = $1 AND revision = $5 \
         RETURNING public_id::text",
    )
    .bind(key)
    .bind(name)
    .bind(enabled)
    .bind(actor.database_user_id())
    .bind(expected_revision)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("update library"))?;
    let Some(public_id) = public_id else {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM volund.library_roots WHERE root_key=$1)",
        )
        .bind(key)
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error("check library existence"))?;
        return Err(if exists {
            LibraryAdminError::Conflict("library changed; reload before retrying".into())
        } else {
            LibraryAdminError::NotFound
        });
    };
    audit(
        &mut transaction,
        actor,
        "library.update",
        &public_id,
        json!({"key": key, "nameChanged": name.is_some(), "enabled": enabled}),
    )
    .await?;
    let query = format!("{MANAGED_LIBRARY_SELECT} WHERE root.root_key = $1");
    let row = sqlx::query(&query)
        .bind(key)
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error("load updated library"))?;
    let result = managed_library(&row);
    transaction
        .commit()
        .await
        .map_err(database_error("commit library update"))?;
    Ok(result)
}

/// Enqueue one durable scan and record actor-aware audit evidence.
///
/// # Errors
///
/// Returns not-found, disabled, or database errors.
pub async fn scan(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    key: &str,
    full: bool,
    confirmation: Option<&str>,
) -> Result<ScanResult, LibraryAdminError> {
    if full
        && validate_confirmation(
            &format!("FULL SCAN {key}"),
            confirmation.unwrap_or_default(),
        )
        .is_err()
    {
        crate::security_audit::denied(
            pool,
            actor,
            "library.scan.full",
            "library",
            "confirmation_mismatch",
        )
        .await;
        return Err(LibraryAdminError::BadRequest(
            "exact library action confirmation is required".to_owned(),
        ));
    }
    let library = load(pool, key).await?;
    if !library.enabled {
        return Err(LibraryAdminError::Conflict(
            "disabled libraries cannot be scanned".into(),
        ));
    }
    if library.storage.state == OperationalState::Blocked {
        return Err(LibraryAdminError::Conflict(
            "blocked library storage cannot be scanned".into(),
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin library scan"))?;
    // Serialize admission with library activation changes, retaining the lock
    // until the queue mutation and its audit have committed together.
    let enabled: bool =
        sqlx::query_scalar("SELECT enabled FROM volund.library_roots WHERE root_key=$1 FOR SHARE")
            .bind(key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error("lock library scan admission"))?
            .ok_or(LibraryAdminError::NotFound)?;
    if !enabled {
        return Err(LibraryAdminError::Conflict(
            "disabled libraries cannot be scanned".into(),
        ));
    }
    let row = sqlx::query(
        "INSERT INTO volund.scan_runs (library_root_id, status, full_scan) \
         SELECT id, 'queued', $2 FROM volund.library_roots WHERE root_key = $1 \
         ON CONFLICT (library_root_id) WHERE status IN ('queued', 'running') \
         DO UPDATE SET full_scan = volund.scan_runs.full_scan OR EXCLUDED.full_scan \
         RETURNING public_id::text, status, full_scan",
    )
    .bind(key)
    .bind(full)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error("enqueue library scan"))?;
    let result = ScanResult {
        id: row.get(0),
        status: row.get(1),
        full: row.get(2),
    };
    audit_scan(&mut transaction, actor, &library.id, key, &result).await?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit library scan"))?;
    Ok(result)
}

fn validate_confirmation(expected: &str, actual: &str) -> Result<(), LibraryAdminError> {
    if actual == expected {
        Ok(())
    } else {
        Err(LibraryAdminError::BadRequest(
            "exact library action confirmation is required".to_owned(),
        ))
    }
}

async fn load(pool: &PgPool, key: &str) -> Result<ManagedLibrary, LibraryAdminError> {
    let query = format!("{MANAGED_LIBRARY_SELECT} WHERE root.root_key = $1");
    sqlx::query(&query)
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(database_error("load library"))?
        .as_ref()
        .map(managed_library)
        .ok_or(LibraryAdminError::NotFound)
}

fn managed_library(row: &sqlx::postgres::PgRow) -> ManagedLibrary {
    let filesystem_path: String = row.get(3);
    ManagedLibrary {
        id: row.get(0),
        key: row.get(1),
        name: row.get(2),
        filesystem_path: filesystem_path.clone(),
        read_only: row.get(4),
        enabled: row.get(5),
        file_count: row.get(6),
        missing_file_count: row.get(7),
        latest_scan_status: row.get(8),
        latest_scan_started_at_unix_ms: row.get(9),
        updated_at_unix_ms: row.get(10),
        revision: row.get(11),
        storage: storage_health::probe(std::path::Path::new(&filesystem_path)),
    }
}

fn validate_requested_path(path: &str) -> Result<PathBuf, LibraryAdminError> {
    if path.is_empty() || path != path.trim() || path.len() > 4096 {
        return Err(LibraryAdminError::BadRequest(
            "library path must contain 1 to 4096 characters without surrounding whitespace".into(),
        ));
    }
    if !path.starts_with('/') {
        return Err(LibraryAdminError::BadRequest(
            "library path must be an absolute Unix path".into(),
        ));
    }
    Ok(PathBuf::from(path))
}

fn validate_key(key: &str) -> Result<(), LibraryAdminError> {
    let valid = (1..=63).contains(&key.len())
        && key.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || (index > 0 && (byte.is_ascii_digit() || byte == b'_' || byte == b'-'))
        });
    if valid {
        Ok(())
    } else {
        Err(LibraryAdminError::BadRequest(
            "library key must start with a lowercase letter and contain only lowercase letters, digits, '_' or '-'".into(),
        ))
    }
}

fn validate_name(name: &str) -> Result<&str, LibraryAdminError> {
    if name == name.trim() && (1..=160).contains(&name.len()) {
        Ok(name)
    } else {
        Err(LibraryAdminError::BadRequest(
            "library name must contain 1 to 160 characters without surrounding whitespace".into(),
        ))
    }
}

#[cfg(unix)]
fn has_write_permission(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o222 != 0
}

#[cfg(not(unix))]
fn has_write_permission(metadata: &fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}

async fn audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    target_id: &str,
    metadata: serde_json::Value,
) -> Result<(), LibraryAdminError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id, metadata) \
         VALUES ($1, $2::uuid, $3, $4, 'success', 'library', $5::uuid, $6)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(action)
    .bind(target_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await
    .map_err(database_error("audit library mutation"))?;
    Ok(())
}

async fn audit_scan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    target_id: &str,
    key: &str,
    result: &ScanResult,
) -> Result<(), LibraryAdminError> {
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, \
          target_type, target_public_id, metadata) \
         VALUES ($1, $2::uuid, $3, 'library.scan.queued', 'success', 'library', $4::uuid, $5)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(target_id)
    .bind(json!({
        "key": key,
        "scanId": result.id,
        "full": result.full,
        "status": result.status
    }))
    .execute(&mut **transaction)
    .await
    .map_err(database_error("audit library scan"))?;
    Ok(())
}

fn conflict_or_database(error: &sqlx::Error, context: &'static str) -> LibraryAdminError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        LibraryAdminError::Conflict("library key or filesystem path is already registered".into())
    } else {
        LibraryAdminError::Database(format!("{context}: {error}"))
    }
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> LibraryAdminError {
    move |error| LibraryAdminError::Database(format!("{context}: {error}"))
}

const MANAGED_LIBRARY_SELECT: &str = "SELECT root.public_id::text, root.root_key, root.display_name, root.filesystem_path, \
     root.read_only, root.enabled, \
     (SELECT count(*)::bigint FROM volund.source_files source WHERE source.library_root_id = root.id), \
     (SELECT count(*)::bigint FROM volund.source_files source WHERE source.library_root_id = root.id AND source.missing_at IS NOT NULL), \
     (SELECT scan.status FROM volund.scan_runs scan WHERE scan.library_root_id = root.id ORDER BY scan.id DESC LIMIT 1), \
     (SELECT (extract(epoch FROM scan.started_at) * 1000)::bigint FROM volund.scan_runs scan WHERE scan.library_root_id = root.id ORDER BY scan.id DESC LIMIT 1), \
     (extract(epoch FROM root.updated_at) * 1000)::bigint, root.revision FROM volund.library_roots root";

#[cfg(test)]
mod tests {
    use super::{LibraryAdminError, validate_key, validate_name, validate_requested_path};

    #[test]
    fn stable_library_key_contract_rejects_unsafe_values() {
        assert!(validate_key("cad-main_2").is_ok());
        assert!(matches!(
            validate_key("CAD"),
            Err(LibraryAdminError::BadRequest(_))
        ));
        assert!(matches!(
            validate_key("2cad"),
            Err(LibraryAdminError::BadRequest(_))
        ));
    }

    #[test]
    fn names_and_paths_reject_surrounding_whitespace_and_non_unix_paths() {
        assert!(validate_name("CAD Archive").is_ok());
        assert!(validate_name(" CAD Archive").is_err());
        assert!(validate_requested_path("/srv/cad").is_ok());
        assert!(validate_requested_path("C:\\cad").is_err());
        assert!(validate_requested_path(" /srv/cad").is_err());
    }
}
