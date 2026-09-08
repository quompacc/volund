use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::response::IntoResponse;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::api_models::UpdateSettingRequest;
use crate::session::AuthenticatedSession;
use crate::settings;

pub(crate) async fn list(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    settings::list(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::Settings)
}

pub(crate) async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(setting_key): Path<String>,
    Json(request): Json<UpdateSettingRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_settings_admin(&actor)?;
    settings::update(
        &state.pool,
        &actor,
        &setting_key,
        request.value,
        request.expected_revision,
        request.confirmation.as_deref(),
    )
    .await
    .map(Json)
    .map_err(ApiError::Settings)
}
