use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::library_admin;
use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLibraryRequest {
    key: String,
    name: String,
    path: String,
    confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLibraryRequest {
    expected_revision: i64,
    name: Option<String>,
    enabled: Option<bool>,
    confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateLibraryPathRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartLibraryScanRequest {
    #[serde(default)]
    full: bool,
    confirmation: Option<String>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    library_admin::list(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::Library)
}

pub(crate) async fn validate(
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<ValidateLibraryPathRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    library_admin::validate_path(&request.path)
        .map(Json)
        .map_err(ApiError::Library)
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<CreateLibraryRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    library_admin::create(
        &state.pool,
        &actor,
        &request.key,
        &request.name,
        &request.path,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(ApiError::Library)
}

pub(crate) async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(root_key): Path<String>,
    Json(request): Json<UpdateLibraryRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    library_admin::update(
        &state.pool,
        &actor,
        &root_key,
        request.expected_revision,
        request.name.as_deref(),
        request.enabled,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(ApiError::Library)
}

pub(crate) async fn scan(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(root_key): Path<String>,
    Json(request): Json<StartLibraryScanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    let result = library_admin::scan(
        &state.pool,
        &actor,
        &root_key,
        request.full,
        request.confirmation.as_deref(),
    )
    .await
    .map_err(ApiError::Library)?;
    Ok((StatusCode::ACCEPTED, Json(result)))
}
