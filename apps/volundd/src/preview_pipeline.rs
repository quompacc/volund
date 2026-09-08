use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use sqlx::{PgPool, Row};
use tokio::process::Command;
use volund_core::{CAD_CONVERT_CONTRACT_VERSION, normalize_library_path};

use crate::file_hash::sha256_file;

const DEFAULT_DERIVED_ROOT: &str = "/srv/volund/derived";
const DEFAULT_SCRATCH_ROOT: &str = "/srv/volund/scratch";
const DEFAULT_CONVERTER: &str = "/usr/local/bin/volund-cad-convert";
const DEFAULT_TIMEOUT_SECONDS: u64 = 1800;

#[derive(Clone, Debug)]
pub struct PreviewWorkerConfig {
    pub runner: PathBuf,
    pub converter: PathBuf,
    pub derived_root: PathBuf,
    pub scratch_root: PathBuf,
    pub timeout_seconds: u64,
}

impl PreviewWorkerConfig {
    /// Load native preview-worker paths and limits from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable current executable, relative paths,
    /// or a timeout outside 1..=86400 seconds.
    pub fn from_environment() -> Result<Self, String> {
        let timeout = env::var("VOLUND_CONVERSION_TIMEOUT_SECONDS").ok().map_or(
            Ok(DEFAULT_TIMEOUT_SECONDS),
            |value| {
                value
                    .parse()
                    .map_err(|_| "VOLUND_CONVERSION_TIMEOUT_SECONDS must be an integer".to_owned())
            },
        )?;
        Self::new(
            env::current_exe()
                .map_err(|error| format!("cannot resolve volundd executable: {error}"))?,
            env::var_os("VOLUND_CONVERTER")
                .map_or_else(|| PathBuf::from(DEFAULT_CONVERTER), PathBuf::from),
            env::var_os("VOLUND_DERIVED_ROOT")
                .map_or_else(|| PathBuf::from(DEFAULT_DERIVED_ROOT), PathBuf::from),
            env::var_os("VOLUND_SCRATCH_ROOT")
                .map_or_else(|| PathBuf::from(DEFAULT_SCRATCH_ROOT), PathBuf::from),
            timeout,
        )
    }

    /// Build explicit worker settings for isolated integration tests.
    ///
    /// # Errors
    ///
    /// Returns an error for non-absolute paths or an invalid timeout.
    pub fn new(
        runner: PathBuf,
        converter: PathBuf,
        derived_root: PathBuf,
        scratch_root: PathBuf,
        timeout_seconds: u64,
    ) -> Result<Self, String> {
        for (name, path) in [
            ("runner", &runner),
            ("converter", &converter),
            ("derived root", &derived_root),
            ("scratch root", &scratch_root),
        ] {
            if !path.to_string_lossy().starts_with('/') {
                return Err(format!("{name} must be an absolute Unix path"));
            }
        }
        if !(1..=86_400).contains(&timeout_seconds) {
            return Err("conversion timeout must be between 1 and 86400".to_owned());
        }
        Ok(Self {
            runner,
            converter,
            derived_root,
            scratch_root,
            timeout_seconds,
        })
    }
}

#[derive(Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ProcessReport {
    pub id: String,
    pub status: String,
}

struct ClaimedRun {
    id: i64,
    public_id: String,
    content_id: i64,
    sha256: String,
    profile: String,
    linear_deflection: Option<f64>,
    angular_deflection: Option<f64>,
}

/// Enqueue or reuse the latest suitable preview request for a source file.
///
/// # Errors
///
/// Returns an error for an unknown/missing source or a database failure.
pub async fn enqueue(
    pool: &PgPool,
    file_id: &str,
    profile: &str,
) -> Result<PreviewRequest, String> {
    enqueue_internal(pool, file_id, profile, true).await
}

/// Force a new bounded conversion run for explicit thumbnail regeneration.
///
/// # Errors
/// Returns an error for an unknown source, invalid profile, active duplicate, or database failure.
pub async fn regenerate(
    pool: &PgPool,
    file_id: &str,
    profile: &str,
) -> Result<PreviewRequest, String> {
    enqueue_internal(pool, file_id, profile, false).await
}

async fn enqueue_internal(
    pool: &PgPool,
    file_id: &str,
    profile: &str,
    reuse_ready: bool,
) -> Result<PreviewRequest, String> {
    let resolved = crate::conversion_profiles::resolve(pool, profile).await?;
    let row = sqlx::query(
        "SELECT content.id FROM volund.source_files source \
         JOIN volund.content_objects content ON content.id = source.content_object_id \
         WHERE source.public_id::text = $1 AND source.missing_at IS NULL \
         AND source.lifecycle_state='available'",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot find preview source: {error}"))?
    .ok_or_else(|| format!("unknown or missing source file: {file_id}"))?;
    let content_id: i64 = row.get(0);
    let contract_version = i32::try_from(CAD_CONVERT_CONTRACT_VERSION)
        .map_err(|_| "CAD converter contract version is out of range".to_owned())?;
    if reuse_ready {
        if let Some(existing) = sqlx::query(
            "WITH candidate AS (SELECT id FROM volund.conversion_runs \
         WHERE content_object_id = $1 AND conversion_profile_id = $2 AND conversion_profile_revision=$5 \
         AND converter_name = 'volund-cad-convert' AND converter_version = $3 \
         AND contract_version = $4 AND status = 'ready' \
         AND EXISTS (SELECT 1 FROM volund.derived_artifacts WHERE conversion_run_id=volund.conversion_runs.id) \
         ORDER BY id DESC FOR UPDATE LIMIT 1) UPDATE volund.conversion_runs run SET artifact_protected_until=now()+interval '10 minutes' FROM candidate WHERE run.id=candidate.id RETURNING run.public_id::text,run.status",
    )
    .bind(content_id)
    .bind(resolved.database_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(contract_version)
    .bind(resolved.revision)
    .fetch_optional(pool)
    .await
        .map_err(|error| format!("cannot find reusable preview: {error}"))?
        {
            return Ok(PreviewRequest {
                id: existing.get(0),
                status: existing.get(1),
            });
        }
    }
    let row = sqlx::query(
        "INSERT INTO volund.conversion_runs \
         (content_object_id, converter_name, converter_version, contract_version, profile, status,conversion_profile_id,conversion_profile_revision,profile_snapshot,settings) \
         VALUES ($1, 'volund-cad-convert', $2, $3, $4, 'queued',$5,$6,$7,$7) \
         ON CONFLICT (content_object_id, conversion_profile_id, conversion_profile_revision) WHERE status IN ('queued', 'running') \
         DO UPDATE SET content_object_id = EXCLUDED.content_object_id \
         RETURNING public_id::text, status",
    )
    .bind(content_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(contract_version)
    .bind(&resolved.native_preset)
    .bind(resolved.database_id)
    .bind(resolved.revision)
    .bind(&resolved.snapshot)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot enqueue preview: {error}"))?;
    Ok(PreviewRequest {
        id: row.get(0),
        status: row.get(1),
    })
}

/// Claim and execute at most one queued preview conversion.
///
/// # Errors
///
/// Returns an error when claiming or persisting a job fails. Conversion errors
/// are durably recorded and returned as a successful `failed` report.
pub async fn process_next(
    pool: &PgPool,
    config: &PreviewWorkerConfig,
) -> Result<Option<ProcessReport>, String> {
    let Some(run) = claim_next(pool).await? else {
        return Ok(None);
    };
    let outcome = execute_claimed(pool, config, &run).await;
    match outcome {
        Ok(()) => Ok(Some(ProcessReport {
            id: run.public_id,
            status: "ready".to_owned(),
        })),
        Err(message) => {
            if message == "conversion cancellation requested" {
                mark_cancelled(pool, run.id).await?;
            } else {
                mark_failed(pool, run.id, &message).await?;
            }
            Ok(Some(ProcessReport {
                id: run.public_id,
                status: if message == "conversion cancellation requested" {
                    "cancelled".to_owned()
                } else {
                    "failed".to_owned()
                },
            }))
        }
    }
}

/// Process one bounded parallel batch. # Errors Returns policy, claim, task, or worker failure.
#[allow(clippy::missing_errors_doc)]
pub async fn process_batch(
    pool: &PgPool,
    config: &PreviewWorkerConfig,
) -> Result<Vec<ProcessReport>, String> {
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load conversion concurrency".to_owned())?
        .conversion_concurrency;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..limit {
        let pool = pool.clone();
        let config = config.clone();
        tasks.spawn(async move { process_next(&pool, &config).await });
    }
    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Some(report) =
            result.map_err(|error| format!("preview worker task failed: {error}"))??
        {
            reports.push(report);
        }
    }
    Ok(reports)
}

async fn claim_next(pool: &PgPool) -> Result<Option<ClaimedRun>, String> {
    sqlx::query(
        "UPDATE volund.conversion_runs SET \
         status = CASE WHEN cancellation_requested_at IS NULL THEN 'failed' ELSE 'cancelled' END, \
         finished_at = now(), diagnostics = CASE WHEN cancellation_requested_at IS NULL \
         THEN jsonb_build_array(jsonb_build_object('message', 'stale worker recovered')) ELSE '[]'::jsonb END \
         WHERE status = 'running' AND started_at < now() - interval '2 hours'",
    )
    .execute(pool)
    .await
    .map_err(|error| format!("cannot recover stale preview jobs: {error}"))?;
    let limit = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load conversion concurrency".to_owned())?
        .conversion_concurrency;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin preview claim: {error}"))?;
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,2)")
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("cannot lock preview claim: {error}"))?;
    let running: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.conversion_runs WHERE status='running'")
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("cannot count preview claims: {error}"))?;
    if running >= limit {
        tx.rollback().await.ok();
        return Ok(None);
    }
    let row = sqlx::query(
        "UPDATE volund.conversion_runs run SET status = 'running', started_at = now(), \
         converter_version = $1 WHERE run.id = ( \
          SELECT queued.id FROM volund.conversion_runs queued \
          WHERE queued.status = 'queued' AND queued.cancellation_requested_at IS NULL \
          ORDER BY queued.requested_at, queued.id \
          FOR UPDATE SKIP LOCKED LIMIT 1) \
         RETURNING run.id, run.public_id::text, run.content_object_id, \
          (SELECT sha256 FROM volund.content_objects WHERE id = run.content_object_id), run.profile, \
          (run.profile_snapshot->>'linearDeflection')::double precision, \
          (run.profile_snapshot->>'angularDeflection')::double precision",
    )
    .bind(env!("CARGO_PKG_VERSION"))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("cannot claim preview job: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("cannot commit preview claim: {error}"))?;
    Ok(row.map(|row| ClaimedRun {
        id: row.get(0),
        public_id: row.get(1),
        content_id: row.get(2),
        sha256: row.get(3),
        profile: row.get(4),
        linear_deflection: row.get(5),
        angular_deflection: row.get(6),
    }))
}

async fn execute_claimed(
    pool: &PgPool,
    config: &PreviewWorkerConfig,
    run: &ClaimedRun,
) -> Result<(), String> {
    let input = find_verified_source(pool, run).await?;
    let mut command = Command::new(&config.runner);
    command
        .arg("convert")
        .arg("--input")
        .arg(&input)
        .arg("--profile")
        .arg(&run.profile)
        .arg("--timeout-seconds")
        .arg(config.timeout_seconds.to_string())
        .arg("--derived-root")
        .arg(&config.derived_root)
        .arg("--scratch-root")
        .arg(&config.scratch_root)
        .arg("--converter")
        .arg(&config.converter)
        .kill_on_drop(true);
    if let Some(value) = run.linear_deflection {
        command.arg("--linear-deflection").arg(value.to_string());
    }
    if let Some(value) = run.angular_deflection {
        command.arg("--angular-deflection").arg(value.to_string());
    }
    let output = crate::preview_process::run(command, pool, run.id, config.timeout_seconds).await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "guarded conversion failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let ready_dir = stdout
        .lines()
        .find_map(|line| line.strip_prefix("output="))
        .map(PathBuf::from)
        .ok_or_else(|| "guarded conversion did not report its output directory".to_owned())?;
    persist_ready(pool, config, run, &ready_dir).await
}

async fn find_verified_source(pool: &PgPool, run: &ClaimedRun) -> Result<PathBuf, String> {
    let rows = sqlx::query(
        "SELECT root.filesystem_path, source.relative_path FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id = source.library_root_id \
         WHERE source.content_object_id = $1 AND source.missing_at IS NULL \
         AND source.lifecycle_state='available' \
         ORDER BY source.id",
    )
    .bind(run.content_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot find conversion source: {error}"))?;
    for row in rows {
        let path = PathBuf::from(row.get::<String, _>(0)).join(row.get::<String, _>(1));
        if path.is_file() && sha256_file(&path).is_ok_and(|hash| hash.as_str() == run.sha256) {
            return Ok(path);
        }
    }
    Err("no available source still matches the indexed content hash".to_owned())
}

async fn persist_ready(
    pool: &PgPool,
    config: &PreviewWorkerConfig,
    run: &ClaimedRun,
    ready_dir: &Path,
) -> Result<(), String> {
    let canonical_root = fs::canonicalize(&config.derived_root)
        .map_err(|error| format!("cannot resolve derived root: {error}"))?;
    let canonical_ready = fs::canonicalize(ready_dir)
        .map_err(|error| format!("cannot resolve conversion output: {error}"))?;
    let relative_dir = canonical_ready
        .strip_prefix(&canonical_root)
        .map_err(|_| "conversion output escaped the derived root".to_owned())?;
    let artifacts = [
        ("preview-glb", "preview.glb", "model/gltf-binary"),
        ("thumbnail-raster", "thumbnail.png", "image/png"),
        ("assembly-manifest", "assembly.json", "application/json"),
        ("diagnostics", "diagnostics.json", "application/json"),
        ("result", "result.json", "application/json"),
    ];
    let mut prepared = Vec::new();
    for (kind, name, media_type) in artifacts {
        let path = canonical_ready.join(name);
        let size = i64::try_from(
            fs::metadata(&path)
                .map_err(|error| format!("missing conversion artifact {name}: {error}"))?
                .len(),
        )
        .map_err(|_| format!("conversion artifact is too large: {name}"))?;
        let relative = relative_dir.join(name).to_string_lossy().replace('\\', "/");
        prepared.push((
            kind,
            normalize_library_path(&relative)?,
            sha256_file(&path)?,
            size,
            media_type,
        ));
    }
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let cancelled: bool = sqlx::query_scalar(
        "SELECT cancellation_requested_at IS NOT NULL FROM volund.conversion_runs WHERE id = $1 FOR UPDATE",
    )
    .bind(run.id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| format!("cannot lock conversion completion: {error}"))?;
    if cancelled {
        sqlx::query(
            "UPDATE volund.conversion_runs SET status = 'cancelled', finished_at = now(), diagnostics = '[]'::jsonb WHERE id = $1",
        )
        .bind(run.id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("cannot complete conversion cancellation: {error}"))?;
        transaction
            .commit()
            .await
            .map_err(|error| error.to_string())?;
        return Err("conversion cancellation requested".to_owned());
    }
    for (kind, relative, hash, size, media_type) in prepared {
        sqlx::query(
            "INSERT INTO volund.derived_artifacts \
             (conversion_run_id, artifact_kind, relative_path, sha256, byte_size, media_type) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(run.id)
        .bind(kind)
        .bind(relative)
        .bind(hash.as_str())
        .bind(size)
        .bind(media_type)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("cannot persist conversion artifact: {error}"))?;
    }
    sqlx::query(
        "UPDATE volund.conversion_runs SET status = 'ready', finished_at = now() WHERE id = $1",
    )
    .bind(run.id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("cannot complete preview job: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

async fn mark_failed(pool: &PgPool, run_id: i64, message: &str) -> Result<(), String> {
    let diagnostics = json!([{ "message": message }]).to_string();
    sqlx::query(
        "UPDATE volund.conversion_runs SET status = 'failed', finished_at = now(), \
         diagnostics = $2::jsonb WHERE id = $1 AND status = 'running' \
         AND cancellation_requested_at IS NULL",
    )
    .bind(run_id)
    .bind(diagnostics)
    .execute(pool)
    .await
    .map_err(|error| format!("cannot persist preview failure: {error}"))?;
    Ok(())
}

async fn mark_cancelled(pool: &PgPool, run_id: i64) -> Result<(), String> {
    sqlx::query(
        "UPDATE volund.conversion_runs SET status = 'cancelled', finished_at = now(), \
         diagnostics = '[]'::jsonb WHERE id = $1 AND status = 'running' \
         AND cancellation_requested_at IS NOT NULL",
    )
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(|error| format!("cannot persist preview cancellation: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_configuration_refuses_relative_paths_and_extreme_timeouts() {
        let valid = PreviewWorkerConfig::new(
            "/usr/local/bin/volundd".into(),
            "/usr/local/bin/volund-cad-convert".into(),
            "/srv/volund/derived".into(),
            "/srv/volund/scratch".into(),
            1800,
        );
        assert!(valid.is_ok());
        assert!(
            PreviewWorkerConfig::new(
                "relative".into(),
                "/converter".into(),
                "/derived".into(),
                "/scratch".into(),
                0,
            )
            .is_err()
        );
    }
}
