use axum::Json;
use axum::Router;
use axum::extract::{Extension, Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get as route_get, post};
use serde::Deserialize;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::job_admin::{self, JobAdminError, JobListOptions};
use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobQuery {
    kind: Option<String>,
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct JobActionRequest {
    confirmation: String,
}

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/jobs", route_get(list))
        .route("/jobs/{kind}/{id}", route_get(get))
        .route("/jobs/{kind}/{id}/cancel", post(cancel))
        .route("/jobs/{kind}/{id}/retry", post(retry))
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Query(query): Query<JobQuery>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    job_admin::list(
        &state.pool,
        JobListOptions {
            kind: query.kind.as_deref(),
            status: query.status.as_deref(),
            limit: query.limit,
            offset: query.offset,
        },
    )
    .await
    .map(Json)
    .map_err(api_error)
}

pub(crate) async fn get(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    job_admin::get(&state.pool, &kind, &id)
        .await
        .map(Json)
        .map_err(api_error)
}

pub(crate) async fn cancel(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((kind, id)): Path<(String, String)>,
    Json(request): Json<JobActionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    job_admin::cancel(&state.pool, &actor, &kind, &id, &request.confirmation)
        .await
        .map(Json)
        .map_err(api_error)
}

pub(crate) async fn retry(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((kind, id)): Path<(String, String)>,
    Json(request): Json<JobActionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    job_admin::retry(&state.pool, &actor, &kind, &id, &request.confirmation)
        .await
        .map(Json)
        .map_err(api_error)
}

fn api_error(error: JobAdminError) -> ApiError {
    match error {
        JobAdminError::BadRequest(message) => ApiError::BadRequest(message),
        JobAdminError::Conflict(message) => ApiError::Conflict(message),
        JobAdminError::NotFound => ApiError::NotFound("job not found".to_owned()),
        JobAdminError::Database(diagnostic) => ApiError::Database(diagnostic),
    }
}

const fn default_limit() -> i64 {
    50
}
