use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Extension, Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::api_models::{
    CancelImportRequest, ConfigureImportRequest, ImportListQuery, ImportPreviewRequest,
    RenameImportRequest, ResolveImportItemRequest,
};
use crate::session::AuthenticatedSession;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/imports", get(list_imports))
        .route("/imports/storage", get(import_storage))
        .route("/imports/preview", post(preview_import))
        .route("/imports/latest-uploaded", get(latest_uploaded_import))
        .route(
            "/imports/{draft_id}",
            get(import_detail).patch(rename_import),
        )
        .route("/imports/{draft_id}/manifest", get(import_manifest))
        .route("/imports/{draft_id}/cancel", post(cancel_import))
        .route("/imports/{draft_id}/metadata", post(configure_import))
        .route("/imports/{draft_id}/review", post(review_import))
        .route(
            "/imports/{draft_id}/items/{item_id}/resolution",
            post(resolve_item),
        )
        .route("/imports/{draft_id}/commit", post(commit_import))
        .route(
            "/imports/{draft_id}/items/{item_id}/content",
            post(upload_import_item).layer(DefaultBodyLimit::disable()),
        )
}

async fn import_manifest(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
) -> Result<Json<crate::api_models::ImportDraftSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_manifest::draft_manifest(&state.pool, &actor, &draft_id)
        .await
        .map(Json)
        .map_err(ApiError::Review)
}

async fn list_imports(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Query(query): Query<ImportListQuery>,
) -> Result<Json<crate::api_models::ImportDraftPage>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_lifecycle::list(
        &state.pool,
        &actor,
        query.status.as_deref(),
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(ApiError::ImportLifecycle)
}

async fn import_detail(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
) -> Result<Json<crate::api_models::ImportDraftLifecycleSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_lifecycle::load_one(&state.pool, &actor, &draft_id)
        .await
        .map(Json)
        .map_err(ApiError::ImportLifecycle)
}

async fn rename_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
    Json(request): Json<RenameImportRequest>,
) -> Result<Json<crate::api_models::ImportDraftLifecycleSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_lifecycle::rename(&state.pool, &actor, &draft_id, &request.display_name)
        .await
        .map(Json)
        .map_err(ApiError::ImportLifecycle)
}

async fn import_storage(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Json<crate::api_models::ImportStorageSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_lifecycle::storage(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::ImportLifecycle)
}

async fn cancel_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
    Json(request): Json<CancelImportRequest>,
) -> Result<Json<crate::api_models::ImportCancelSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_lifecycle::cancel(
        &state.pool,
        &state.incoming_root,
        &actor,
        &draft_id,
        &request.confirmation,
    )
    .await
    .map(Json)
    .map_err(ApiError::ImportLifecycle)
}

async fn preview_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<ImportPreviewRequest>,
) -> Result<Json<crate::api_models::ImportDraftSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_planner::preview_import(&state.pool, &actor, request)
        .await
        .map(Json)
        .map_err(ApiError::Import)
}

async fn configure_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
    Json(request): Json<ConfigureImportRequest>,
) -> Result<Json<crate::api_models::ImportConfigurationSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_configuration::configure_import(&state.pool, &actor, &draft_id, request)
        .await
        .map(Json)
        .map_err(ApiError::Import)
}

async fn latest_uploaded_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Json<Option<crate::api_models::ImportDraftSummary>>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_manifest::latest_uploaded(&state.pool, &actor)
        .await
        .map(Json)
        .map_err(ApiError::Review)
}

async fn review_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
) -> Result<Json<crate::api_models::ImportReviewSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_review::review_import(&state.pool, &actor, &draft_id)
        .await
        .map(Json)
        .map_err(ApiError::Review)
}

async fn resolve_item(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((draft_id, item_id)): Path<(String, String)>,
    Json(request): Json<ResolveImportItemRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_review::resolve_item(
        &state.pool,
        &actor,
        &draft_id,
        &item_id,
        &request.action,
        request.target_path.as_deref(),
    )
    .await
    .map_err(ApiError::Review)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn commit_import(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(draft_id): Path<String>,
) -> Result<Json<crate::api_models::ImportCommitSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::import_commit::commit_import(&state.pool, &state.incoming_root, &draft_id, &actor)
        .await
        .map(Json)
        .map_err(ApiError::Commit)
}

async fn upload_import_item(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((draft_id, item_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<crate::api_models::ImportUploadSummary>, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    let length = headers
        .get(header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| ApiError::BadRequest("invalid Content-Length header".to_owned()))?
                .parse::<u64>()
                .map_err(|_| ApiError::BadRequest("invalid Content-Length header".to_owned()))
        })
        .transpose()?;
    crate::import_upload::upload_item(
        &state.pool,
        &state.incoming_root,
        &actor,
        &draft_id,
        &item_id,
        length,
        body,
    )
    .await
    .map(Json)
    .map_err(ApiError::Upload)
}
