use axum::extract::{Extension, Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::session::AuthenticatedSession;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/models/{model_id}/thumbnail-candidates", get(candidates))
        .route("/models/{model_id}/thumbnail", put(update))
        .route("/models/{model_id}/thumbnail/regenerate", post(regenerate))
}

async fn candidates(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
) -> Result<Json<Vec<crate::thumbnail_catalog::ThumbnailCandidate>>, ApiError> {
    crate::thumbnail_catalog::candidates(&state.pool, &model_id)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}

async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<crate::thumbnail_catalog::ThumbnailUpdate>,
) -> Result<Json<crate::api_models::ModelSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::thumbnail_catalog::update(&state.pool, &actor, &model_id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}

async fn regenerate(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<crate::thumbnail_catalog::ThumbnailRegenerate>,
) -> Result<Json<crate::preview_pipeline::PreviewRequest>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::thumbnail_catalog::regenerate(&state.pool, &actor, &model_id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}
