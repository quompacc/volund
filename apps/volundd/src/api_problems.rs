use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::session::AuthenticatedSession;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/models/{model_id}/problems", get(list).put(update))
}

async fn list(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
) -> Result<Json<Vec<crate::problem_catalog::ModelProblem>>, ApiError> {
    crate::problem_catalog::list(&state.pool, &state.derived_root, &model_id)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}

async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<crate::problem_catalog::ProblemStatusUpdate>,
) -> Result<Json<Vec<crate::problem_catalog::ModelProblem>>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::problem_catalog::set_status(
        &state.pool,
        &state.derived_root,
        &actor,
        &model_id,
        &request,
    )
    .await
    .map(Json)
    .map_err(ApiError::Model)
}
