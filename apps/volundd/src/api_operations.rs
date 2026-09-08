use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::operational_log::{self, Event, Severity};
use crate::operations;
use crate::session::AuthenticatedSession;
use crate::support_bundle;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/operations/health", get(health))
        .route("/operations/support-bundle", post(create_support_bundle))
}

pub(crate) async fn health(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    operations::inspect(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::Database)
}

async fn create_support_bundle(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Response, ApiError> {
    if let Err(error) = api_auth::require_library_admin(&actor) {
        crate::security_audit::denied(
            &state.pool,
            &actor,
            "support-bundle.create",
            "support-bundle",
            "capability_denied",
        )
        .await;
        return Err(error);
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        support_bundle::create(&state.pool, &state.support_root),
    )
    .await;
    let archive = match result {
        Ok(Ok(archive)) => archive,
        Ok(Err(code)) => {
            let _ = support_bundle::audit(&state.pool, &actor, "failure", 0, &code).await;
            let _ = operational_log::record(
                &state.pool,
                &Event {
                    severity: Severity::Warning,
                    event: "support_bundle.create.failed",
                    component: "daemon",
                    code: &code,
                    message: "support bundle generation failed safely",
                    request_id: None,
                    job_id: None,
                    run_id: None,
                    actor_id: Some(&actor.user_id),
                },
            )
            .await;
            return Err(ApiError::BadRequest(code));
        }
        Err(_) => {
            let code = "support_bundle_timeout";
            let _ = support_bundle::audit(&state.pool, &actor, "failure", 0, code).await;
            return Err(ApiError::BadRequest(code.to_owned()));
        }
    };
    support_bundle::audit(
        &state.pool,
        &actor,
        "success",
        archive.bytes.len(),
        "support_bundle_created",
    )
    .await
    .map_err(ApiError::Database)?;
    let message = format!(
        "support archive bytes={} sha256={}",
        archive.bytes.len(),
        archive.sha256
    );
    let _ = operational_log::record(
        &state.pool,
        &Event {
            severity: Severity::Info,
            event: "support_bundle.create.completed",
            component: "daemon",
            code: "support_bundle_created",
            message: &message,
            request_id: None,
            job_id: None,
            run_id: None,
            actor_id: Some(&actor.user_id),
        },
    )
    .await;
    let disposition = format!("attachment; filename=\"{}\"", archive.filename);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition)
                .map_err(|_| ApiError::BadRequest("support_bundle_filename_invalid".to_owned()))?,
        )
        .header("x-content-sha256", archive.sha256)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(archive.bytes))
        .map_err(|_| ApiError::BadRequest("support_bundle_response_failed".to_owned()))
}
