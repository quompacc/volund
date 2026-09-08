use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::api::ApiState;
use crate::api_auth;
use crate::api_error::ApiError;
use crate::author_catalog::{self, AuthorInput, AuthorMerge, AuthorUpdate};
use crate::collection_catalog::{
    self, CollectionInput, CollectionRemoval, CollectionUpdate, MembershipUpdate,
};
use crate::session::AuthenticatedSession;
use crate::tag_catalog::{self, TagInput, TagMerge, TagRemoval, TagUpdate};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    include_merged: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    include_inactive: Option<bool>,
}

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/authors", get(list_authors).post(create_author))
        .route("/authors/{author_id}", get(get_author).put(update_author))
        .route("/authors/{author_id}/merge", post(merge_author))
        .route(
            "/collections",
            get(list_collections).post(create_collection),
        )
        .route(
            "/collections/{collection_id}",
            get(get_collection)
                .put(update_collection)
                .delete(remove_collection),
        )
        .route(
            "/collections/{collection_id}/models/{model_id}",
            axum::routing::put(add_collection_member).delete(remove_collection_member),
        )
        .route("/tags", get(list_tags).post(create_tag))
        .route(
            "/tags/{tag_id}",
            get(get_tag).put(update_tag).delete(remove_tag),
        )
        .route("/tags/{tag_id}/merge", post(merge_tag))
}

async fn list_authors(
    State(state): State<ApiState>,
    Query(query): Query<AuthorQuery>,
) -> Result<impl IntoResponse, ApiError> {
    author_catalog::list(
        &state.pool,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
        query.include_merged.unwrap_or(false),
    )
    .await
    .map(Json)
    .map_err(ApiError::Database)
}

async fn get_author(
    State(state): State<ApiState>,
    Path(author_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    author_catalog::get(&state.pool, &author_id)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("author not found".to_owned()))
}

async fn create_author(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(input): Json<AuthorInput>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    author_catalog::create(&state.pool, &actor, &input)
        .await
        .map(|author| (StatusCode::CREATED, Json(author)))
        .map_err(ApiError::Author)
}

async fn update_author(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(author_id): Path<String>,
    Json(request): Json<AuthorUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    author_catalog::update(&state.pool, &actor, &author_id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Author)
}

async fn merge_author(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(author_id): Path<String>,
    Json(request): Json<AuthorMerge>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    author_catalog::merge(&state.pool, &actor, &author_id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Author)
}

async fn list_collections(
    State(state): State<ApiState>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    collection_catalog::list(
        &state.pool,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(ApiError::Database)
}

async fn get_collection(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    collection_catalog::get(&state.pool, &id)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("collection not found".to_owned()))
}

async fn create_collection(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(input): Json<CollectionInput>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    collection_catalog::create(&state.pool, &actor, &input)
        .await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(ApiError::Collection)
}

async fn update_collection(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<CollectionUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    collection_catalog::update(&state.pool, &actor, &id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Collection)
}

async fn add_collection_member(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((collection_id, model_id)): Path<(String, String)>,
    Json(request): Json<MembershipUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    collection_catalog::set_membership(
        &state.pool,
        &actor,
        &collection_id,
        &model_id,
        request.expected_revision,
        true,
    )
    .await
    .map(Json)
    .map_err(ApiError::Collection)
}

async fn remove_collection_member(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path((collection_id, model_id)): Path<(String, String)>,
    Json(request): Json<MembershipUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_catalog_write(&actor)?;
    collection_catalog::set_membership(
        &state.pool,
        &actor,
        &collection_id,
        &model_id,
        request.expected_revision,
        false,
    )
    .await
    .map(Json)
    .map_err(ApiError::Collection)
}

async fn remove_collection(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<CollectionRemoval>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    collection_catalog::remove(&state.pool, &actor, &id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Collection)
}

async fn list_tags(
    State(state): State<ApiState>,
    Query(query): Query<TagQuery>,
) -> Result<impl IntoResponse, ApiError> {
    tag_catalog::list(
        &state.pool,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
        query.include_inactive.unwrap_or(false),
    )
    .await
    .map(Json)
    .map_err(ApiError::Database)
}

async fn get_tag(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    tag_catalog::get(&state.pool, &id)
        .await
        .map_err(ApiError::Database)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("tag not found".to_owned()))
}

async fn create_tag(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(input): Json<TagInput>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    tag_catalog::create(&state.pool, &actor, &input)
        .await
        .map(|value| (StatusCode::CREATED, Json(value)))
        .map_err(ApiError::Tag)
}

async fn update_tag(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<TagUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    tag_catalog::update(&state.pool, &actor, &id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Tag)
}

async fn merge_tag(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<TagMerge>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    tag_catalog::merge(&state.pool, &actor, &id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Tag)
}

async fn remove_tag(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    Json(request): Json<TagRemoval>,
) -> Result<impl IntoResponse, ApiError> {
    api_auth::require_metadata_admin(&actor)?;
    tag_catalog::remove(&state.pool, &actor, &id, &request)
        .await
        .map(Json)
        .map_err(ApiError::Tag)
}
