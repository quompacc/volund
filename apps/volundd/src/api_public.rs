use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_models::{CreateFirstOwnerRequest, HealthResponse, SetupStatusResponse};
use crate::identity::{self, FirstOwnerInput};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/health", get(health))
        .route("/setup", get(setup_status))
        .route("/setup/owner", post(create_first_owner))
        .route(
            "/invitations/accept",
            post(crate::api_user::accept_invitation),
        )
        .route("/sessions", post(crate::api_session::login))
        .route("/slicer-download/{token}", get(slicer_download))
}

async fn slicer_download(
    State(state): State<ApiState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    crate::slicer_handoff::download(state, token, headers).await
}

async fn setup_status(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let initialized = identity::setup_complete(&state.pool)
        .await
        .map_err(ApiError::Identity)?;
    Ok(Json(SetupStatusResponse {
        initialized,
        bootstrap_available: !initialized && state.identity.bootstrap_available(),
    }))
}

async fn create_first_owner(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateFirstOwnerRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let secret = state
        .identity
        .bootstrap_secret()
        .ok_or(ApiError::SetupUnavailable)?;
    let token = headers
        .get("x-volund-setup-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    identity::create_first_owner(
        &state.pool,
        &secret,
        token,
        FirstOwnerInput {
            email: request.email,
            display_name: request.display_name,
            password: request.password,
        },
    )
    .await
    .map(Json)
    .map_err(ApiError::Identity)
}

async fn health(State(state): State<ApiState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|error| ApiError::Database(format!("health query failed: {error}")))?;
    Ok(Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    }))
}
