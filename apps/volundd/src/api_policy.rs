use std::path::PathBuf;

use axum::Json;
use axum::Router;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::conversion_profiles::{self, ProfileInput};
use crate::retention;
use crate::scheduler::{self, ScheduleInput};
use crate::session::AuthenticatedSession;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdate {
    expected_revision: i64,
    confirmation: String,
    #[serde(flatten)]
    input: ProfileInput,
}
#[derive(Deserialize)]
pub struct ProfileCreate {
    confirmation: String,
    #[serde(flatten)]
    input: ProfileInput,
}
#[derive(Deserialize)]
pub struct Confirmation {
    confirmation: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionConfirmation {
    confirmation: String,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleUpdate {
    expected_revision: i64,
    confirmation: String,
    #[serde(flatten)]
    input: ScheduleInput,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCreate {
    confirmation: String,
    #[serde(flatten)]
    input: ScheduleInput,
}

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/conversion-profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/conversion-profiles/{profile_id}",
            axum::routing::put(update_profile).delete(retire_profile),
        )
        .route("/scan-schedules", get(list_schedules).post(create_schedule))
        .route(
            "/scan-schedules/{schedule_id}",
            axum::routing::put(update_schedule).delete(delete_schedule),
        )
        .route("/retention/preview", get(retention_preview))
        .route("/retention/runs", post(run_retention))
}

pub(crate) async fn list_profiles(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    conversion_profiles::list(&state.pool)
        .await
        .map(Json)
        .map_err(policy_error)
}
pub(crate) async fn create_profile(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<ProfileCreate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    conversion_profiles::create(&state.pool, &actor, &request.input, &request.confirmation)
        .await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(policy_error)
}
pub(crate) async fn update_profile(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<ProfileUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    conversion_profiles::update(
        &state.pool,
        &actor,
        &id,
        request.expected_revision,
        &request.input,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(policy_error)
}
pub(crate) async fn retire_profile(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<RevisionConfirmation>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    conversion_profiles::retire(
        &state.pool,
        &actor,
        &id,
        request.expected_revision,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(policy_error)
}

pub(crate) async fn list_schedules(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    scheduler::list(&state.pool)
        .await
        .map(Json)
        .map_err(policy_error)
}
pub(crate) async fn create_schedule(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<ScheduleCreate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    scheduler::create(&state.pool, &actor, &request.input, &request.confirmation)
        .await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(policy_error)
}
pub(crate) async fn update_schedule(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<ScheduleUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    scheduler::update(
        &state.pool,
        &actor,
        &id,
        request.expected_revision,
        &request.input,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(policy_error)
}
pub(crate) async fn delete_schedule(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<RevisionConfirmation>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    scheduler::delete(
        &state.pool,
        &actor,
        &id,
        request.expected_revision,
        &request.confirmation,
    )
    .await
    .map(|()| StatusCode::NO_CONTENT)
    .map_err(policy_error)
}

pub(crate) async fn retention_preview(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    retention::preview(&state.pool)
        .await
        .map(Json)
        .map_err(policy_error)
}
pub(crate) async fn run_retention(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<Confirmation>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    let root = derived_root();
    retention::execute(
        &state.pool,
        &root,
        Some(&actor),
        Some(&request.confirmation),
    )
    .await
    .map(Json)
    .map_err(policy_error)
}

pub(crate) fn derived_root() -> PathBuf {
    std::env::var_os("VOLUND_DERIVED_ROOT")
        .map_or_else(|| PathBuf::from("/srv/volund/derived"), PathBuf::from)
}
fn policy_error(message: String) -> ApiError {
    if [
        "must",
        "required",
        "not found",
        "revision",
        "confirmation",
        "retired",
        "active library",
        "invalid",
        "between",
    ]
    .iter()
    .any(|term| message.contains(term))
    {
        ApiError::BadRequest(message)
    } else {
        ApiError::Database(message)
    }
}
