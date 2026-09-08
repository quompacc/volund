use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::database;
use crate::operational_log;
use crate::operations;
use crate::session::AuthenticatedSession;
use crate::settings;

const MAX_ARCHIVE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MEMBERS: usize = 8;
static BUNDLE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct SupportArchive {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestMember {
    name: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format: &'static str,
    version: &'static str,
    created_at: String,
    categories: Vec<String>,
    members: Vec<ManifestMember>,
}

/// Create, atomically publish, read, and remove one bounded support archive.
///
/// # Errors
///
/// Returns a safe stable diagnostic when collection or filesystem checks fail.
pub async fn create(pool: &PgPool, root: &Path) -> Result<SupportArchive, String> {
    validate_root(root)?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut members = collect_members(pool).await?;
    if members.len() >= MAX_MEMBERS {
        return Err("support_bundle_category_limit".to_owned());
    }
    let manifest = Manifest {
        format: "volund-support-tar-v1",
        version: env!("CARGO_PKG_VERSION"),
        created_at: created_at.clone(),
        categories: members.iter().map(|(name, _)| name.clone()).collect(),
        members: members
            .iter()
            .map(|(name, bytes)| ManifestMember {
                name: name.clone(),
                bytes: bytes.len(),
                sha256: sha256(bytes),
            })
            .collect(),
    };
    members.push(("manifest.json".to_owned(), json_bytes(&manifest)?));
    let archive = tar_archive(&members)?;
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err("support_bundle_size_limit".to_owned());
    }

    let sequence = BUNDLE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stem = format!("bundle-{}-{sequence}", std::process::id());
    let partial = root.join(format!(".{stem}.partial"));
    let published = root.join(format!("{stem}.tar"));
    let cleanup = Cleanup::new(partial.clone(), published.clone());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|_| "support_bundle_create_failed".to_owned())?;
    file.write_all(&archive)
        .and_then(|()| file.sync_all())
        .map_err(|_| "support_bundle_write_failed".to_owned())?;
    drop(file);
    fs::rename(&partial, &published).map_err(|_| "support_bundle_publish_failed".to_owned())?;
    let bytes = fs::read(&published).map_err(|_| "support_bundle_read_failed".to_owned())?;
    if bytes.len() != archive.len() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("support_bundle_verification_failed".to_owned());
    }
    drop(cleanup);
    let timestamp = created_at
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect::<String>();
    Ok(SupportArchive {
        sha256: sha256(&bytes),
        bytes,
        filename: format!("volund-support-{timestamp}.tar"),
    })
}

/// Record support generation without archive contents or secrets.
///
/// # Errors
///
/// Returns a safe error when the security audit cannot be written.
pub async fn audit(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    outcome: &str,
    byte_count: usize,
    code: &str,
) -> Result<(), String> {
    let bytes = i64::try_from(byte_count).unwrap_or(i64::MAX);
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,metadata) \
         VALUES ($1,$2::uuid,$3,'support-bundle.create',$4,'support-bundle', \
         jsonb_build_object('bytes',$5::bigint,'code',$6::text))",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(outcome)
    .bind(bytes)
    .bind(code)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| "support_bundle_audit_failed".to_owned())
}

async fn collect_members(pool: &PgPool) -> Result<Vec<(String, Vec<u8>)>, String> {
    let database = database::inspect(pool).await?;
    let build = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "apiContractVersion": 1,
        "nativeRuntime": "debian-systemd",
    });
    let database_state = json!({
        "serverVersion": database.server_version_num,
        "schemaTables": database.schema_table_count,
        "expectedSchemaTables": database::EXPECTED_SCHEMA_TABLES,
        "appliedMigrations": database.applied_migrations,
        "expectedMigrations": database::EXPECTED_MIGRATIONS,
        "ready": database.is_ready(),
    });
    let mut health = serde_json::to_value(operations::inspect(pool).await?)
        .map_err(|_| "support_bundle_health_encode_failed".to_owned())?;
    sanitize_value(&mut health, None);
    let listed_settings = settings::list(pool)
        .await
        .map_err(|_| "support_bundle_settings_read_failed".to_owned())?;
    let mut effective_settings = serde_json::to_value(listed_settings)
        .map_err(|_| "support_bundle_settings_encode_failed".to_owned())?;
    sanitize_value(&mut effective_settings, None);
    let jobs = job_counts(pool).await?;
    let policy = policy_status(pool).await?;
    let logs = operational_log::recent(pool).await?;
    Ok(vec![
        ("build.json".to_owned(), json_bytes(&build)?),
        ("database.json".to_owned(), json_bytes(&database_state)?),
        ("health.json".to_owned(), json_bytes(&health)?),
        ("jobs.json".to_owned(), json_bytes(&jobs)?),
        ("policy.json".to_owned(), json_bytes(&policy)?),
        ("settings.json".to_owned(), json_bytes(&effective_settings)?),
        ("logs.json".to_owned(), json_bytes(&logs)?),
    ])
}

async fn job_counts(pool: &PgPool) -> Result<Value, String> {
    let rows = sqlx::query(
        "SELECT kind,status,count(*)::bigint FROM ( \
         SELECT 'scan'::text kind,status FROM volund.scan_runs \
         UNION ALL SELECT 'conversion'::text kind,status FROM volund.conversion_runs \
         ) jobs GROUP BY kind,status ORDER BY kind,status",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| "support_bundle_job_status_failed".to_owned())?;
    Ok(Value::Array(
        rows.iter()
            .map(|row| {
                json!({
                    "kind": row.get::<String, _>(0),
                    "status": row.get::<String, _>(1),
                    "count": row.get::<i64, _>(2),
                })
            })
            .collect(),
    ))
}

async fn policy_status(pool: &PgPool) -> Result<Value, String> {
    let schedules: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.scan_schedules")
        .fetch_one(pool)
        .await
        .map_err(|_| "support_bundle_schedule_status_failed".to_owned())?;
    let profiles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.conversion_profiles WHERE enabled")
            .fetch_one(pool)
            .await
            .map_err(|_| "support_bundle_profile_status_failed".to_owned())?;
    let retention = sqlx::query(
        "SELECT status,artifact_runs,artifact_bytes,diagnostics_cleared, \
         to_char(started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
         FROM volund.retention_runs ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| "support_bundle_retention_status_failed".to_owned())?
    .map(|row| {
        json!({
            "status": row.get::<String, _>(0),
            "artifactRuns": row.get::<i64, _>(1),
            "artifactBytes": row.get::<i64, _>(2),
            "diagnosticsCleared": row.get::<i64, _>(3),
            "startedAt": row.get::<String, _>(4),
        })
    });
    let imports = sqlx::query(
        "SELECT status,count(*)::bigint,coalesce(sum(total_files),0)::bigint, \
         coalesce(sum(uploaded_bytes),0)::bigint FROM volund.import_drafts \
         GROUP BY status ORDER BY status",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| "support_bundle_import_status_failed".to_owned())?
    .iter()
    .map(|row| {
        json!({"status":row.get::<String,_>(0),"drafts":row.get::<i64,_>(1),
            "items":row.get::<i64,_>(2),"uploadedBytes":row.get::<i64,_>(3)})
    })
    .collect::<Vec<_>>();
    let import_cleanup = sqlx::query(
        "SELECT status,expired_drafts,cleaned_drafts,cleaned_bytes FROM volund.import_cleanup_runs \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| "support_bundle_import_cleanup_failed".to_owned())?
    .map(|row| {
        json!({"status":row.get::<String,_>(0),"expiredDrafts":row.get::<i64,_>(1),
            "cleanedDrafts":row.get::<i64,_>(2),"cleanedBytes":row.get::<i64,_>(3)})
    });
    Ok(json!({
        "scanSchedules": schedules,
        "enabledConversionProfiles": profiles,
        "lastRetentionRun": retention,
        "imports": imports,
        "lastImportCleanup": import_cleanup,
    }))
}

fn validate_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("support_bundle_root_not_absolute".to_owned());
    }
    if let Ok(metadata) = fs::symlink_metadata(root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("support_bundle_root_unsafe".to_owned());
        }
    }
    fs::create_dir_all(root).map_err(|_| "support_bundle_root_unavailable".to_owned())?;
    let canonical = fs::canonicalize(root)
        .map_err(|_| "support_bundle_root_canonicalization_failed".to_owned())?;
    if canonical != root {
        return Err("support_bundle_root_alias_refused".to_owned());
    }
    Ok(())
}

fn sanitize_value(value: &mut Value, key: Option<&str>) {
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                sanitize_value(child, Some(child_key));
            }
        }
        Value::Array(values) => {
            for child in values {
                sanitize_value(child, key);
            }
        }
        Value::String(text) => {
            let sensitive_key = key.is_some_and(|name| {
                let lower = name.to_ascii_lowercase();
                lower.contains("path")
                    || lower.contains("password")
                    || lower.contains("token")
                    || lower.contains("secret")
                    || lower.contains("cookie")
                    || lower.contains("authorization")
            });
            *text = if sensitive_key {
                "<redacted>".to_owned()
            } else {
                operational_log::sanitize(text)
            };
        }
        _ => {}
    }
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "support_bundle_json_encode_failed".to_owned())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn tar_archive(members: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut archive = Vec::new();
    for (name, content) in members {
        if name.len() > 99 || name.contains("..") || name.starts_with('/') || name.contains('\\') {
            return Err("support_bundle_member_name_refused".to_owned());
        }
        let mut header = [0_u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        write_octal(&mut header[100..108], 0o600)?;
        write_octal(&mut header[108..116], 0)?;
        write_octal(&mut header[116..124], 0)?;
        write_octal(
            &mut header[124..136],
            u64::try_from(content.len()).unwrap_or(u64::MAX),
        )?;
        let mtime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |value| value.as_secs());
        write_octal(&mut header[136..148], mtime)?;
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[265..271].copy_from_slice(b"volund");
        header[297..303].copy_from_slice(b"volund");
        let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
        let encoded = format!("{checksum:06o}\0 ");
        header[148..156].copy_from_slice(encoded.as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(content);
        let padding = (512 - content.len() % 512) % 512;
        archive.resize(archive.len() + padding, 0);
        if archive.len() > MAX_ARCHIVE_BYTES {
            return Err("support_bundle_size_limit".to_owned());
        }
    }
    archive.resize(archive.len() + 1024, 0);
    Ok(archive)
}

fn write_octal(field: &mut [u8], value: u64) -> Result<(), String> {
    let digits = format!("{value:o}");
    if digits.len() + 1 > field.len() {
        return Err("support_bundle_tar_value_overflow".to_owned());
    }
    field.fill(b'0');
    let start = field.len() - digits.len() - 1;
    field[start..start + digits.len()].copy_from_slice(digits.as_bytes());
    field[field.len() - 1] = 0;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

struct Cleanup {
    partial: PathBuf,
    published: PathBuf,
}

impl Cleanup {
    const fn new(partial: PathBuf, published: PathBuf) -> Self {
        Self { partial, published }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.partial);
        let _ = fs::remove_file(&self.published);
    }
}

#[cfg(test)]
mod tests {
    use super::{sanitize_value, tar_archive};
    use serde_json::json;

    #[test]
    fn tar_members_and_json_redaction_refuse_unsafe_content() {
        assert!(tar_archive(&[("../secret".to_owned(), vec![1])]).is_err());
        assert!(tar_archive(&[("safe.json".to_owned(), vec![1, 2, 3])]).is_ok());
        let mut value = json!({
            "filesystemPath": "/srv/private/library",
            "message": "Authorization: Bearer secret",
            "state": "healthy"
        });
        sanitize_value(&mut value, None);
        assert_eq!(value["filesystemPath"], "<redacted>");
        assert_eq!(value["message"], "diagnostic redacted");
        assert_eq!(value["state"], "healthy");
    }

    #[cfg(unix)]
    #[test]
    fn support_root_refuses_symlink() {
        use super::validate_root;
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("volund-support-root-{}", std::process::id()));
        let target = root.with_extension("target");
        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).expect("create target");
        symlink(&target, &root).expect("create symlink");
        assert!(validate_root(&root).is_err());
        std::fs::remove_file(&root).expect("remove symlink");
        std::fs::remove_dir_all(&target).expect("remove target");
    }
}
