use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::api_models::{
    AcceptInvitationRequest, ChangeOwnPasswordRequest, CreateUserRequest, InviteUserRequest,
    ResetUserPasswordRequest, UpdateUserRequest,
};
use crate::session::AuthenticatedSession;
use crate::user_admin::{self, CreateUserInput, UpdateUserInput};
use crate::user_invitation::{self, InviteUserInput};

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_user_admin(&actor)?;
    user_admin::list_users(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::User)
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_user_admin(&actor)?;
    user_admin::create_user(
        &state.pool,
        &actor,
        CreateUserInput {
            email: request.email,
            display_name: request.display_name,
            role: request.role,
            password: request.password,
        },
    )
    .await
    .map(Json)
    .map_err(ApiError::User)
}

pub(crate) async fn invite(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<InviteUserRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_user_admin(&actor)?;
    user_invitation::invite_user(
        &state.pool,
        &actor,
        InviteUserInput {
            email: request.email,
            display_name: request.display_name,
            role: request.role,
        },
    )
    .await
    .map(Json)
    .map_err(ApiError::User)
}

pub(crate) async fn accept_invitation(
    State(state): State<ApiState>,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<StatusCode, ApiError> {
    user_invitation::accept_invitation(&state.pool, &request.token, request.password)
        .await
        .map_err(ApiError::User)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn change_own_password(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<ChangeOwnPasswordRequest>,
) -> Result<StatusCode, ApiError> {
    user_admin::change_own_password(
        &state.pool,
        &actor,
        request.current_password,
        request.new_password,
    )
    .await
    .map_err(ApiError::User)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn update(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(user_id): Path<String>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_user_admin(&actor)?;
    user_admin::update_user(
        &state.pool,
        &actor,
        &user_id,
        UpdateUserInput {
            display_name: request.display_name,
            role: request.role,
            status: request.status,
        },
    )
    .await
    .map(Json)
    .map_err(ApiError::User)
}

pub(crate) async fn reset_password(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(user_id): Path<String>,
    Json(request): Json<ResetUserPasswordRequest>,
) -> Result<StatusCode, ApiError> {
    api_auth::require_user_admin(&actor)?;
    user_admin::reset_password(
        &state.pool,
        &actor,
        &user_id,
        request.password,
        request.must_change,
    )
    .await
    .map_err(ApiError::User)?;
    Ok(StatusCode::NO_CONTENT)
}
