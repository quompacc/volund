//! Saved model list views: named, per-account search filters (ADR 0032).

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::model_search::{ModelSearch, ModelSearchQuery};
use crate::session::AuthenticatedSession;

/// Views one account may keep.
pub const MAX_VIEWS_PER_ACCOUNT: i64 = 50;

/// The stored filter; paging is deliberately not part of a view.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSavedView {
    pub name: String,
    pub query: ViewQuery,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedView {
    pub id: String,
    pub name: String,
    pub query: ViewQuery,
    pub created_at_unix_ms: i64,
}

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/saved-views", get(list).post(create))
        .route("/saved-views/{view_id}", delete(remove))
}

/// Normalize a view through the same validation as a live search, so a saved
/// view can never hold a query the search endpoint would reject.
fn normalize(query: ViewQuery) -> Result<ViewQuery, ApiError> {
    let search = ModelSearch::parse(ModelSearchQuery {
        q: query.q,
        tag: query.tag,
        kind: query.kind,
        sort: query.sort,
        limit: None,
        offset: None,
    })
    .map_err(ApiError::BadRequest)?;
    let sort = match search.sort {
        crate::model_search::ModelSort::Updated => None,
        crate::model_search::ModelSort::Name => Some("name".to_owned()),
        crate::model_search::ModelSort::Created => Some("created".to_owned()),
    };
    Ok(ViewQuery {
        q: search.text,
        tag: search.tag,
        kind: search.kind,
        sort,
    })
}

async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<Json<Vec<SavedView>>, ApiError> {
    let rows = sqlx::query(
        "SELECT public_id::text, name, query, round(extract(epoch FROM created_at)*1000)::bigint \
         FROM volund.saved_model_views WHERE user_id=$1 ORDER BY lower(name), id",
    )
    .bind(actor.database_user_id())
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot list saved views: {error}")))?;
    rows.iter()
        .map(|row| {
            Ok(SavedView {
                id: row.get(0),
                name: row.get(1),
                query: serde_json::from_value(row.get(2))
                    .map_err(|error| ApiError::Database(format!("invalid saved view: {error}")))?,
                created_at_unix_ms: row.get(3),
            })
        })
        .collect::<Result<_, _>>()
        .map(Json)
}

async fn create(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<CreateSavedView>,
) -> Result<(StatusCode, Json<SavedView>), ApiError> {
    let name = request.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "view name must be 1 to 80 printable characters".to_owned(),
        ));
    }
    let query = normalize(request.query)?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.saved_model_views WHERE user_id=$1")
            .bind(actor.database_user_id())
            .fetch_one(&state.pool)
            .await
            .map_err(|error| ApiError::Database(format!("cannot count saved views: {error}")))?;
    if count >= MAX_VIEWS_PER_ACCOUNT {
        return Err(ApiError::TooMany(
            "saved_view_limit",
            format!("at most {MAX_VIEWS_PER_ACCOUNT} saved views per account"),
        ));
    }
    let document = serde_json::to_value(&query)
        .map_err(|error| ApiError::Database(format!("cannot encode saved view: {error}")))?;
    let row = sqlx::query(
        "INSERT INTO volund.saved_model_views (user_id,name,query) VALUES ($1,$2,$3) \
         ON CONFLICT DO NOTHING \
         RETURNING public_id::text, round(extract(epoch FROM created_at)*1000)::bigint",
    )
    .bind(actor.database_user_id())
    .bind(&name)
    .bind(document)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot save view: {error}")))?
    .ok_or_else(|| ApiError::Conflict(format!("a saved view named '{name}' already exists")))?;
    Ok((
        StatusCode::CREATED,
        Json(SavedView {
            id: row.get(0),
            name,
            query,
            created_at_unix_ms: row.get(1),
        }),
    ))
}

async fn remove(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(view_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let removed =
        sqlx::query("DELETE FROM volund.saved_model_views WHERE public_id::text=$1 AND user_id=$2")
            .bind(&view_id)
            .bind(actor.database_user_id())
            .execute(&state.pool)
            .await
            .map_err(|error| ApiError::Database(format!("cannot remove saved view: {error}")))?
            .rows_affected();
    if removed == 0 {
        return Err(ApiError::NotFound("saved view not found".to_owned()));
    }
    Ok(StatusCode::NO_CONTENT)
}
