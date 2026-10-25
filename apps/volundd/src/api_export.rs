//! HTTP access to portable exports (ADR 0029).

use std::path::{Path as FsPath, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio_util::io::ReaderStream;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::metadata_export;
use crate::model_export;
use crate::session::AuthenticatedSession;

/// Exports copy whole originals; two at a time keep disk and I/O bounded.
static EXPORT_SLOTS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/models/{model_id}/export", get(export_model))
        .route("/exports/metadata", post(export_metadata))
}

async fn export_model(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
) -> Result<Response, ApiError> {
    let Ok(_slot) = EXPORT_SLOTS.try_acquire() else {
        return Err(ApiError::Conflict("model_export_busy".to_owned()));
    };
    let plan = model_export::plan(&state.pool, &model_id)
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound("model not found".to_owned()))?;
    crate::support_bundle::validate_root(&state.support_root).map_err(ApiError::BadRequest)?;
    let staging = Staging::new(&state.support_root);
    let created_at = chrono::Utc::now().timestamp_millis();
    let destination = staging.path.clone();
    let archive_plan = plan.clone();
    let written = tokio::task::spawn_blocking(move || {
        model_export::write_archive(&destination, &archive_plan, created_at)
    })
    .await
    .unwrap_or_else(|_| Err("model_export_write_failed".to_owned()));
    let summary = match written {
        Ok(summary) => summary,
        Err(code) => {
            let public_code = code
                .split(':')
                .next()
                .unwrap_or("model_export_write_failed");
            let _ = audit(&state.pool, &actor, &model_id, "failure", public_code, None).await;
            return Err(ApiError::Conflict(code));
        }
    };
    // Open, then unlink: the stream keeps the inode alive and nothing remains
    // in the support directory, even if the client disconnects.
    let file = tokio::fs::File::open(&staging.path)
        .await
        .map_err(|_| ApiError::BadRequest("model_export_stage_failed".to_owned()))?;
    drop(staging);
    audit(
        &state.pool,
        &actor,
        &model_id,
        "success",
        "model_exported",
        Some(summary),
    )
    .await
    .map_err(ApiError::Database)?;
    let disposition = format!("attachment; filename=\"{}\"", export_filename(&plan.slug));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_LENGTH, summary.bytes)
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition)
                .map_err(|_| ApiError::BadRequest("model_export_filename_invalid".to_owned()))?,
        )
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-volund-export-format", model_export::FORMAT)
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|_| ApiError::BadRequest("model_export_response_failed".to_owned()))
}

async fn export_metadata(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Response, ApiError> {
    if let Err(error) = api_auth::require_library_admin(&actor) {
        crate::security_audit::denied(
            &state.pool,
            &actor,
            "metadata.export",
            "instance",
            "capability_denied",
        )
        .await;
        return Err(error);
    }
    let created_at = chrono::Utc::now();
    let document = metadata_export::build(&state.pool, created_at.timestamp_millis())
        .await
        .map_err(ApiError::Database)?;
    let body = serde_json::to_vec_pretty(&document)
        .map_err(|_| ApiError::BadRequest("metadata_export_serialization_failed".to_owned()))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events          (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,metadata)          VALUES ($1,$2::uuid,$3,'metadata.export','success','instance',$4)",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(serde_json::json!({ "bytes": body.len(), "counts": document["counts"] }))
    .execute(&state.pool)
    .await
    .map_err(|_| ApiError::Database("metadata_export_audit_failed".to_owned()))?;
    let disposition = format!(
        "attachment; filename=\"volund-metadata-{}.json\"",
        created_at.format("%Y%m%d-%H%M%S")
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition)
                .map_err(|_| ApiError::BadRequest("metadata_export_filename_invalid".to_owned()))?,
        )
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-volund-export-format", metadata_export::FORMAT)
        .body(Body::from(body))
        .map_err(|_| ApiError::BadRequest("metadata_export_response_failed".to_owned()))
}

/// ASCII-only download name derived from the model slug.
pub(crate) fn export_filename(slug: &str) -> String {
    let stem: String = slug
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-');
    format!(
        "{}-volund-export.zip",
        if stem.is_empty() { "model" } else { stem }
    )
}

async fn audit(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    outcome: &str,
    code: &str,
    summary: Option<model_export::ArchiveSummary>,
) -> Result<(), String> {
    let (bytes, included, excluded) = summary.map_or((0, 0, 0), |summary| {
        (
            i64::try_from(summary.bytes).unwrap_or(i64::MAX),
            i64::try_from(summary.included_files).unwrap_or(i64::MAX),
            i64::try_from(summary.excluded_files).unwrap_or(i64::MAX),
        )
    });
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata) \
         VALUES ($1,$2::uuid,$3,'model.export',$4,'model',$5::uuid, \
         jsonb_build_object('code',$6::text,'bytes',$7::bigint,'includedFiles',$8::bigint,'excludedFiles',$9::bigint))",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(outcome)
    .bind(model_id)
    .bind(code)
    .bind(bytes)
    .bind(included)
    .bind(excluded)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| "model_export_audit_failed".to_owned())
}

/// Temporary archive path that is removed when dropped.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn new(root: &FsPath) -> Self {
        let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            path: root.join(format!(
                ".model-export-{}-{sequence}.partial",
                std::process::id()
            )),
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::export_filename;

    #[test]
    fn download_names_are_ascii_and_never_empty() {
        assert_eq!(
            export_filename("voron-trident"),
            "voron-trident-volund-export.zip"
        );
        assert_eq!(
            export_filename("bügel_halter"),
            "b-gel-halter-volund-export.zip"
        );
        assert_eq!(export_filename("ÄÖÜ"), "model-volund-export.zip");
    }
}
