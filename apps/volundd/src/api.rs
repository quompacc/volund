use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use sqlx::PgPool;
use std::path::PathBuf;
use std::str::FromStr;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use volund_core::ContentHash;

use crate::api_auth;
use crate::api_error::ApiError;
use crate::api_models::{
    AddModelComponentRequest, CreateModelRequest, CreateSlicerHandoffRequest,
    EnqueuePreviewRequest, MoveFileRequest, MoveFileResponse, RemoveModelComponentRequest,
    UpdateModelFileRequest, UpdateModelRequest,
};
use crate::api_query::{CatalogQuery, PageQuery};
use crate::artifact_stream;
use crate::catalog_filter::CatalogFilter;
use crate::catalog_queries;
use crate::identity::IdentityConfig;
use crate::model_catalog;
use crate::preview_pipeline;
use crate::session::{AuthenticatedSession, SessionConfig};
use crate::source_move;
use crate::source_stream;

const DEFAULT_PAGE_LIMIT: i64 = 50;
const MAX_PAGE_LIMIT: i64 = 200;

#[derive(Clone)]
pub(crate) struct ApiState {
    pub pool: PgPool,
    pub derived_root: PathBuf,
    pub incoming_root: PathBuf,
    pub support_root: PathBuf,
    pub identity: IdentityConfig,
    pub session: SessionConfig,
}

/// Build the versioned catalog HTTP API with a shared database pool.
pub fn router(pool: PgPool) -> Router {
    router_with_roots(
        pool,
        "/srv/volund/derived".into(),
        "/usr/share/volund/web".into(),
    )
}

/// Build the API and browser application with explicit storage roots.
pub fn router_with_roots(pool: PgPool, derived_root: PathBuf, web_root: PathBuf) -> Router {
    router_with_identity(pool, derived_root, web_root, IdentityConfig::default())
}

/// Build the API with explicit storage roots and identity configuration.
pub fn router_with_identity(
    pool: PgPool,
    derived_root: PathBuf,
    web_root: PathBuf,
    identity: IdentityConfig,
) -> Router {
    let support_root = derived_root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/srv/volund"))
        .join("support");
    router_with_security(
        pool,
        derived_root,
        web_root,
        support_root,
        identity,
        SessionConfig::default(),
    )
}

/// Build the API with explicit identity and browser-session configuration.
pub fn router_with_security(
    pool: PgPool,
    derived_root: PathBuf,
    web_root: PathBuf,
    support_root: PathBuf,
    identity: IdentityConfig,
    session: SessionConfig,
) -> Router {
    let index = web_root.join("index.html");
    let incoming_root = derived_root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/srv/volund"))
        .join("incoming");
    let state = ApiState {
        pool,
        derived_root,
        incoming_root,
        support_root,
        identity,
        session,
    };
    let request_state = state.clone();
    let public_api = crate::api_public::routes().route_layer(middleware::from_fn_with_state(
        state.clone(),
        api_auth::require_public_origin,
    ));
    let protected_api = protected_routes(state.clone());
    let api = public_api.merge(protected_api).fallback(not_found);
    Router::new()
        .nest("/api/v1", api)
        .fallback_service(ServeDir::new(web_root).fallback(ServeFile::new(index)))
        .with_state(state)
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'",
            ),
        ))
        .layer(middleware::from_fn_with_state(
            request_state,
            crate::operational_log::request_id,
        ))
}

#[allow(clippy::too_many_lines)] // Declarative route inventory remains easier to audit as one chain.
fn protected_routes(state: ApiState) -> Router<ApiState> {
    Router::new()
        .route("/sessions", get(crate::api_session::list))
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(crate::api_session::revoke),
        )
        .route(
            "/account/password",
            axum::routing::put(crate::api_user::change_own_password),
        )
        .route(
            "/users",
            get(crate::api_user::list).post(crate::api_user::create),
        )
        .route(
            "/users/{user_id}",
            axum::routing::patch(crate::api_user::update),
        )
        .route("/users/invitations", post(crate::api_user::invite))
        .route(
            "/users/{user_id}/password",
            post(crate::api_user::reset_password),
        )
        .route("/settings", get(crate::api_settings::list))
        .merge(crate::api_preferences::routes())
        .merge(crate::api_primary::routes())
        .merge(crate::api_problems::routes())
        .merge(crate::api_operations::routes())
        .merge(crate::api_policy::routes())
        .merge(crate::api_jobs::routes())
        .merge(crate::api_metadata::routes())
        .merge(crate::api_history::routes())
        .merge(crate::api_thumbnail::routes())
        .route(
            "/settings/{setting_key}",
            axum::routing::put(crate::api_settings::update),
        )
        .route(
            "/session",
            get(crate::api_session::current).delete(crate::api_session::logout),
        )
        .route("/models", get(models).post(create_model))
        .route("/models/{model_id}", get(model).patch(update_model))
        .route("/lifecycle/preview", post(lifecycle_preview))
        .route("/lifecycle/plans/{plan_id}/apply", post(lifecycle_apply))
        .route("/quarantines", get(quarantines))
        .route("/models/{model_id}/files", get(model_files))
        .route(
            "/models/{model_id}/files/{file_id}",
            axum::routing::patch(update_model_file),
        )
        .route("/slicer-targets", get(slicer_targets))
        .route(
            "/models/{model_id}/files/{file_id}/slicer-handoff",
            post(create_slicer_handoff),
        )
        .route(
            "/models/{model_id}/components",
            get(model_components).post(add_model_component),
        )
        .route(
            "/models/{model_id}/components/{child_model_id}",
            axum::routing::delete(remove_model_component),
        )
        .merge(crate::api_import::routes())
        .route("/roots", get(roots))
        .route("/roots/{root_key}/files", get(files))
        .route("/roots/{root_key}/folders", get(folders))
        .route("/roots/{root_key}/scans", get(scans))
        .route(
            "/libraries",
            get(crate::api_library::list).post(crate::api_library::create),
        )
        .route("/libraries/validate", post(crate::api_library::validate))
        .route(
            "/libraries/{root_key}",
            axum::routing::patch(crate::api_library::update),
        )
        .route(
            "/libraries/{root_key}/scans",
            post(crate::api_library::scan),
        )
        .route("/content/{sha256}", get(content))
        .route(
            "/files/{file_id}/previews",
            get(previews).post(enqueue_preview),
        )
        .route("/files/{file_id}/content", get(source_stream::serve))
        .route(
            "/artifacts/{artifact_id}/content",
            get(artifact_stream::serve_by_id),
        )
        .route("/files/{file_id}/move", post(move_file))
        .route(
            "/previews/{preview_id}/artifacts/{kind}",
            get(artifact_stream::serve),
        )
        .route_layer(middleware::from_fn_with_state(
            state,
            api_auth::require_session,
        ))
}

async fn roots(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    catalog_queries::list_roots(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::Database)
}

async fn models(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    model_catalog::list_models(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::Database)
}

async fn model(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    model_catalog::get_model(&state.pool, &model_id)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("model not found".to_owned()))
}

async fn model_files(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    model_catalog::list_model_files_page(&state.pool, &model_id, limit, offset)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("model not found".to_owned()))
}

async fn update_model_file(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((model_id, file_id)): Path<(String, String)>,
    Json(request): Json<UpdateModelFileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::file_metadata::update(&state.pool, &actor, &model_id, &file_id, request)
        .await
        .map_err(ApiError::Model)?;
    model_catalog::list_model_files(&state.pool, &model_id)
        .await
        .map_err(ApiError::Database)?
        .and_then(|files| files.into_iter().find(|file| file.id == file_id))
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("model file relationship not found".to_owned()))
}

async fn slicer_targets() -> Result<impl IntoResponse, ApiError> {
    crate::slicer_handoff::targets().map(Json)
}

async fn create_slicer_handoff(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((model_id, file_id)): Path<(String, String)>,
    Json(request): Json<CreateSlicerHandoffRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::slicer_handoff::create(&state, &actor, &model_id, &file_id, request)
        .await
        .map(Json)
}

async fn create_model(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<CreateModelRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::model_maintenance::create_model(&state.pool, &actor, request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}

async fn lifecycle_preview(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<crate::lifecycle::PreviewRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let result = crate::lifecycle::preview(&state.pool, &actor, &request).await;
    if let Err(error) = &result {
        crate::security_audit::denied_target(
            &state.pool,
            &actor,
            &request.action,
            crate::lifecycle::target_type(&request.action),
            &request.target_id,
            error.code(),
        )
        .await;
    }
    result.map(Json).map_err(ApiError::Lifecycle)
}

async fn quarantines(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_library_admin(&actor)?;
    crate::source_quarantine::list(
        &state.pool,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(ApiError::Database)
}

async fn lifecycle_apply(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(plan_id): Path<String>,
    Json(request): Json<crate::lifecycle::ApplyRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let plan: Option<(String, String, String)> = sqlx::query_as(
        "SELECT action,target_type,target_public_id::text FROM volund.lifecycle_plans \
         WHERE public_id::text=$1 AND actor_user_id=$2",
    )
    .bind(&plan_id)
    .bind(actor.database_user_id())
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot resolve lifecycle plan: {error}")))?;
    let (action, target_type, target_id) =
        plan.ok_or_else(|| ApiError::NotFound("lifecycle plan not found".to_owned()))?;
    let result = if action.starts_with("source.") {
        crate::source_quarantine::apply(&state.pool, &actor, &plan_id, &request).await
    } else {
        crate::lifecycle::apply_catalog(&state.pool, &actor, &plan_id, &request).await
    };
    if let Err(error) = &result {
        crate::security_audit::denied_target(
            &state.pool,
            &actor,
            &action,
            &target_type,
            &target_id,
            error.code(),
        )
        .await;
    }
    result.map(Json).map_err(ApiError::Lifecycle)
}

async fn update_model(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    crate::model_maintenance::update_model(&state.pool, &actor, &model_id, request)
        .await
        .map(Json)
        .map_err(ApiError::Model)
}

async fn model_components(
    State(state): State<ApiState>,
    Path(model_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    model_catalog::list_components(&state.pool, &model_id)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("model not found".to_owned()))
}

async fn add_model_component(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(model_id): Path<String>,
    Json(request): Json<AddModelComponentRequest>,
) -> Result<StatusCode, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    model_catalog::add_component(
        &state.pool,
        &actor,
        &model_id,
        &request.child_model_id,
        request.expected_revision,
    )
    .await
    .map_err(ApiError::Model)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_model_component(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((model_id, child_model_id)): Path<(String, String)>,
    Json(request): Json<RemoveModelComponentRequest>,
) -> Result<StatusCode, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    model_catalog::remove_component(
        &state.pool,
        &actor,
        &model_id,
        &child_model_id,
        request.expected_revision,
    )
    .await
    .map_err(ApiError::Model)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn files(
    State(state): State<ApiState>,
    Path(root_key): Path<String>,
    Query(query): Query<CatalogQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let filter = catalog_filter(&query)?;
    catalog_queries::list_files(&state.pool, &root_key, &filter)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("unknown library root: {root_key}")))
}

async fn folders(
    State(state): State<ApiState>,
    Path(root_key): Path<String>,
    Query(query): Query<CatalogQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let filter = catalog_filter(&query)?;
    catalog_queries::list_folders(&state.pool, &root_key, &filter)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("unknown library root: {root_key}")))
}

async fn scans(
    State(state): State<ApiState>,
    Path(root_key): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (limit, offset) = pagination(&query)?;
    catalog_queries::list_scans(&state.pool, &root_key, limit, offset)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("unknown library root: {root_key}")))
}

async fn content(
    State(state): State<ApiState>,
    Path(raw_sha256): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let sha256 = ContentHash::from_str(&raw_sha256)
        .map_err(|message| ApiError::BadRequest(message.to_owned()))?;
    catalog_queries::get_content(&state.pool, sha256.as_str())
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("unknown content hash: {sha256}")))
}

async fn previews(
    State(state): State<ApiState>,
    Path(file_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (limit, offset) = pagination(&query)?;
    catalog_queries::list_previews(&state.pool, &file_id, limit, offset)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("unknown source file: {file_id}")))
}

async fn enqueue_preview(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(file_id): Path<String>,
    Json(request): Json<EnqueuePreviewRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    if request.profile != "web" && request.profile != "fine" {
        return Err(ApiError::BadRequest(
            "preview profile must be web or fine".to_owned(),
        ));
    }
    preview_pipeline::enqueue(&state.pool, &file_id, &request.profile)
        .await
        .map(Json)
        .map_err(|message| {
            if message.starts_with("unknown or missing source file") {
                ApiError::NotFound(message)
            } else {
                ApiError::Database(message)
            }
        })
}

async fn move_file(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(file_id): Path<String>,
    Json(request): Json<MoveFileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    let moved = source_move::move_file(&state.pool, &file_id, &request.destination_directory)
        .await
        .map_err(ApiError::Move)?;
    Ok(Json(MoveFileResponse {
        id: moved.id,
        previous_path: moved.previous_path,
        path: moved.path,
    }))
}

fn pagination(query: &PageQuery) -> Result<(i64, i64), ApiError> {
    bounded_pagination(query.limit, query.offset)
}

fn catalog_filter(query: &CatalogQuery) -> Result<CatalogFilter, ApiError> {
    let (limit, offset) = bounded_pagination(query.limit, query.offset)?;
    CatalogFilter::from_query(query, limit, offset).map_err(ApiError::BadRequest)
}

fn bounded_pagination(limit: Option<i64>, offset: Option<i64>) -> Result<(i64, i64), ApiError> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    let offset = offset.unwrap_or(0);
    if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "limit must be between 1 and {MAX_PAGE_LIMIT}"
        )));
    }
    if offset < 0 {
        return Err(ApiError::BadRequest(
            "offset must not be negative".to_owned(),
        ));
    }
    Ok((limit, offset))
}

async fn not_found() -> ApiError {
    ApiError::NotFound("API route not found".to_owned())
}
