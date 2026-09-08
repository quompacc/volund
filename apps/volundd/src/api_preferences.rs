use axum::Json;
use axum::Router;
use axum::extract::{Extension, State};
use axum::response::IntoResponse;

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::session::AuthenticatedSession;
use crate::user_preferences::{self, UpdateUserPreferences};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/preferences", axum::routing::get(get).put(update))
}

pub(crate) async fn get(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    user_preferences::get(&state.pool, &actor)
        .await
        .map(Json)
        .map_err(ApiError::Settings)
}

pub(crate) async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<UpdateUserPreferences>,
) -> Result<impl IntoResponse, ApiError> {
    user_preferences::update(&state.pool, &actor, request)
        .await
        .map(Json)
        .map_err(ApiError::Settings)
}
