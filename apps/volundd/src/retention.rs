#![allow(clippy::missing_errors_doc)]
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPreview {
    pub artifact_runs: i64,
    pub artifact_files: i64,
    pub artifact_bytes: i64,
    pub diagnostics: i64,
    pub reason_codes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionResult {
    pub id: String,
    pub status: String,
    pub artifact_runs: i64,
    pub artifact_files: i64,
    pub artifact_bytes: i64,
    pub diagnostics_cleared: i64,
    pub result_codes: Vec<String>,
}

/// Preview without mutation. # Errors Returns policy or persistence failure.
pub async fn preview(pool: &PgPool) -> Result<RetentionPreview, String> {
    let policy = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load retention policy".to_owned())?;
    let row = sqlx::query("WITH candidates AS (SELECT run.id FROM volund.conversion_runs run WHERE run.status IN ('ready','partial','failed','cancelled','timed-out') AND (run.artifact_protected_until IS NULL OR run.artifact_protected_until<now()) AND EXISTS (SELECT 1 FROM volund.derived_artifacts artifact WHERE artifact.conversion_run_id=run.id) AND NOT EXISTS (SELECT 1 FROM volund.derived_artifacts selected JOIN volund.models model ON model.thumbnail_artifact_id=selected.id WHERE selected.conversion_run_id=run.id) AND (run.finished_at < now()-make_interval(days=>$1::int) OR run.id NOT IN (SELECT kept.id FROM volund.conversion_runs kept WHERE kept.content_object_id=run.content_object_id AND kept.status='ready' AND EXISTS (SELECT 1 FROM volund.derived_artifacts a WHERE a.conversion_run_id=kept.id) ORDER BY kept.finished_at DESC NULLS LAST,kept.id DESC LIMIT $2))) SELECT count(DISTINCT candidates.id),count(artifact.id),coalesce(sum(artifact.byte_size),0)::bigint FROM candidates JOIN volund.derived_artifacts artifact ON artifact.conversion_run_id=candidates.id")
        .bind(i32::try_from(policy.derived_artifact_days).map_err(|_|"invalid retention age")?).bind(policy.max_artifact_runs)
        .fetch_one(pool).await.map_err(|error|format!("cannot preview retained artifacts: {error}"))?;
    let diagnostics: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM volund.scan_runs WHERE status IN ('completed','failed','cancelled') AND finished_at<now()-make_interval(days=>$1::int) AND error_message IS NOT NULL)+(SELECT count(*) FROM volund.conversion_runs WHERE status IN ('ready','partial','failed','cancelled','timed-out') AND finished_at<now()-make_interval(days=>$1::int) AND diagnostics<>'[]'::jsonb)")
        .bind(i32::try_from(policy.job_diagnostic_days).map_err(|_|"invalid retention age")?).fetch_one(pool).await.map_err(|error|format!("cannot preview retained diagnostics: {error}"))?;
    Ok(RetentionPreview {
        artifact_runs: row.get(0),
        artifact_files: row.get(1),
        artifact_bytes: row.get(2),
        diagnostics,
        reason_codes: vec!["artifact_age_or_count_limit", "diagnostic_age_limit"],
    })
}

/// Execute bounded safe cleanup. # Errors Returns confirmation, path, policy, or persistence failure.
pub async fn execute(
    pool: &PgPool,
    derived_root: &Path,
    actor: Option<&AuthenticatedSession>,
    confirmation: Option<&str>,
) -> Result<RetentionResult, String> {
    if actor.is_some() && confirmation != Some("PURGE DERIVED ARTIFACTS") {
        if let Some(actor) = actor {
            crate::security_audit::denied(
                pool,
                actor,
                "retention.run",
                "retention-run",
                "confirmation_mismatch",
            )
            .await;
        }
        return Err("exact retention confirmation is required".into());
    }
    let root =
        fs::canonicalize(derived_root).map_err(|_| "derived root is unavailable".to_owned())?;
    if fs::symlink_metadata(derived_root)
        .map_err(|_| "derived root is unavailable".to_owned())?
        .file_type()
        .is_symlink()
    {
        return Err("derived root must not be a symlink".into());
    }
    let policy = crate::operational_policy::resolve(pool)
        .await
        .map_err(|_| "cannot load retention policy".to_owned())?;
    let candidates=sqlx::query("SELECT DISTINCT run.id FROM volund.conversion_runs run WHERE run.status IN ('ready','partial','failed','cancelled','timed-out') AND (run.artifact_protected_until IS NULL OR run.artifact_protected_until<now()) AND EXISTS (SELECT 1 FROM volund.derived_artifacts artifact WHERE artifact.conversion_run_id=run.id) AND NOT EXISTS (SELECT 1 FROM volund.derived_artifacts selected JOIN volund.models model ON model.thumbnail_artifact_id=selected.id WHERE selected.conversion_run_id=run.id) AND (run.finished_at < now()-make_interval(days=>$1::int) OR run.id NOT IN (SELECT kept.id FROM volund.conversion_runs kept WHERE kept.content_object_id=run.content_object_id AND kept.status='ready' AND EXISTS (SELECT 1 FROM volund.derived_artifacts a WHERE a.conversion_run_id=kept.id) ORDER BY kept.finished_at DESC NULLS LAST,kept.id DESC LIMIT $2)) ORDER BY run.id")
        .bind(i32::try_from(policy.derived_artifact_days).map_err(|_|"invalid retention age")?).bind(policy.max_artifact_runs).fetch_all(pool).await.map_err(|error|format!("cannot select retention candidates: {error}"))?;
    let mut runs = 0;
    let mut files = 0;
    let mut bytes = 0;
    let mut codes = Vec::new();
    for candidate in candidates {
        let id: i64 = candidate.get(0);
        match purge_run(pool, &root, id).await {
            Ok((f, b)) => {
                runs += 1;
                files += f;
                bytes += b;
            }
            Err(code) => codes.push(code),
        }
    }
    let diagnostic_days =
        i32::try_from(policy.job_diagnostic_days).map_err(|_| "invalid retention age")?;
    let mut final_tx = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin retention result: {error}"))?;
    let scan=sqlx::query("UPDATE volund.scan_runs SET error_message=NULL WHERE status IN ('completed','failed','cancelled') AND finished_at<now()-make_interval(days=>$1::int) AND error_message IS NOT NULL").bind(diagnostic_days).execute(&mut *final_tx).await.map_err(|error|format!("cannot clear scan diagnostics: {error}"))?.rows_affected();
    let conversion=sqlx::query("UPDATE volund.conversion_runs SET diagnostics='[]'::jsonb WHERE status IN ('ready','partial','failed','cancelled','timed-out') AND finished_at<now()-make_interval(days=>$1::int) AND diagnostics<>'[]'::jsonb").bind(diagnostic_days).execute(&mut *final_tx).await.map_err(|error|format!("cannot clear conversion diagnostics: {error}"))?.rows_affected();
    let cleared = i64::try_from(scan + conversion).map_err(|_| "diagnostic count overflow")?;
    if codes.is_empty() {
        codes.push("cleanup_completed".into());
    }
    let status = if codes.iter().all(|code| code == "cleanup_completed") {
        "completed"
    } else {
        "partial"
    };
    let actor_id = actor.map(AuthenticatedSession::database_user_id);
    let id:String=sqlx::query_scalar("INSERT INTO volund.retention_runs (initiated_by,actor_user_id,status,artifact_runs,artifact_files,artifact_bytes,diagnostics_cleared,result_codes) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING public_id::text")
        .bind(if actor.is_some(){"user"}else{"service"}).bind(actor_id).bind(status).bind(runs).bind(files).bind(bytes).bind(cleared).bind(json!(codes)).fetch_one(&mut *final_tx).await.map_err(|error|format!("cannot record retention result: {error}"))?;
    if let Some(actor) = actor {
        sqlx::query("INSERT INTO volund.security_audit_events (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) VALUES ($1,$2::uuid,$3,'retention.run','success','retention-run',$4::uuid,jsonb_build_object('artifactRuns',$5::bigint,'artifactBytes',$6::bigint))").bind(actor.database_user_id()).bind(&actor.user_id).bind(&actor.display_name).bind(&id).bind(runs).bind(bytes).execute(&mut *final_tx).await.map_err(|error|format!("cannot audit retention run: {error}"))?;
    }
    final_tx
        .commit()
        .await
        .map_err(|error| format!("cannot commit retention result: {error}"))?;
    Ok(RetentionResult {
        id,
        status: status.into(),
        artifact_runs: runs,
        artifact_files: files,
        artifact_bytes: bytes,
        diagnostics_cleared: cleared,
        result_codes: codes,
    })
}

async fn purge_run(pool: &PgPool, root: &Path, id: i64) -> Result<(i64, i64), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "database_error".to_owned())?;
    let terminal:bool=sqlx::query_scalar("SELECT status IN ('ready','partial','failed','cancelled','timed-out') AND (artifact_protected_until IS NULL OR artifact_protected_until<now()) FROM volund.conversion_runs WHERE id=$1 FOR UPDATE").bind(id).fetch_optional(&mut *tx).await.map_err(|_|"database_error".to_owned())?.ok_or_else(||"candidate_missing".to_owned())?;
    if !terminal {
        return Err("candidate_became_active".into());
    }
    let artifacts=sqlx::query("SELECT id,relative_path,byte_size FROM volund.derived_artifacts WHERE conversion_run_id=$1 ORDER BY id FOR UPDATE").bind(id).fetch_all(&mut *tx).await.map_err(|_|"database_error".to_owned())?;
    let selected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM volund.models model JOIN volund.derived_artifacts artifact \
         ON artifact.id=model.thumbnail_artifact_id WHERE artifact.conversion_run_id=$1)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "database_error".to_owned())?;
    if selected {
        return Err("selected_thumbnail_protected".to_owned());
    }
    let mut files = 0;
    let mut bytes = 0;
    for artifact in &artifacts {
        let relative: String = artifact.get(1);
        let path = root.join(PathBuf::from(&relative));
        if path.exists() {
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| "artifact_inspection_failed".to_owned())?;
            if metadata.file_type().is_symlink() {
                return Err("artifact_symlink_refused".into());
            }
            let canonical =
                fs::canonicalize(&path).map_err(|_| "artifact_inspection_failed".to_owned())?;
            if !canonical.starts_with(root) {
                return Err("artifact_path_escape_refused".into());
            }
            fs::remove_file(&canonical).map_err(|_| "artifact_remove_failed".to_owned())?;
        }
        files += 1;
        bytes += artifact.get::<i64, _>(2);
    }
    sqlx::query("DELETE FROM volund.derived_artifacts WHERE conversion_run_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "database_error".to_owned())?;
    tx.commit().await.map_err(|_| "database_error".to_owned())?;
    Ok((files, bytes))
}
