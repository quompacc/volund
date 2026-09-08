use axum::extract::{Extension, Path, State};
use axum::routing::put;
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::api_models::SetModelPrimaryRequest;
use crate::session::AuthenticatedSession;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/models/{model_id}/primary", put(update))
}

async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<SetModelPrimaryRequest>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::model_maintenance::update_primary(&state.pool, &actor, &model_id, request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}
