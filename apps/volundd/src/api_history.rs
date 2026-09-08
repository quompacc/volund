use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_query::PageQuery;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/models/{model_id}/history", get(model_history))
        .route("/history", get(catalog_history))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogHistoryQuery {
    target_type: String,
    target_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn catalog_history(
    State(state): State<ApiState>,
    Query(query): Query<CatalogHistoryQuery>,
) -> Result<Json<crate::api_models::Page<crate::catalog_history::HistoryEvent>>, ApiError> {
    crate::catalog_history::list(
        &state.pool,
        &query.target_type,
        &query.target_id,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(ApiError::BadRequest)
}

async fn model_history(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<crate::api_models::Page<crate::model_history::ModelHistoryEvent>>, ApiError> {
    crate::model_history::list(
        &state.pool,
        &model_id,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(ApiError::Model)
}
