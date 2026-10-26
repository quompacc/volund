//! Server-side model list: text search, tag and kind filters, stable sorting,
//! and offset pagination (ADR 0032).

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::api_models::ModelSummary;
use crate::model_catalog::{MODEL_SELECT, model_from_row};

pub const DEFAULT_LIMIT: i64 = 48;
pub const MAX_LIMIT: i64 = 200;
const MAX_QUERY_CHARS: usize = 200;

/// Raw query string of `GET /api/v1/models/search`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSearchQuery {
    pub q: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelSort {
    Updated,
    Name,
    Created,
}

impl ModelSort {
    const fn order_by(self) -> &'static str {
        match self {
            Self::Updated => "model.updated_at DESC,model.name,model.id",
            Self::Name => "lower(model.name),model.name,model.id",
            Self::Created => "model.created_at DESC,model.id DESC",
        }
    }
}

/// A validated model search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSearch {
    pub text: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<String>,
    pub sort: ModelSort,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSearchPage {
    pub items: Vec<ModelSummary>,
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    /// Sum of linked files over every matching model, not only this page.
    pub file_link_total: i64,
}

impl ModelSearch {
    /// Validate a raw query.
    ///
    /// # Errors
    /// Returns a client-facing message for out-of-range or unknown values.
    pub fn parse(query: ModelSearchQuery) -> Result<Self, String> {
        let text = query
            .q
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if text.as_ref().is_some_and(|value| {
            value.chars().count() > MAX_QUERY_CHARS || value.chars().any(char::is_control)
        }) {
            return Err(format!(
                "q must be at most {MAX_QUERY_CHARS} printable characters"
            ));
        }
        let tag = query.tag.filter(|value| !value.is_empty());
        if tag.as_ref().is_some_and(|value| !is_uuid(value)) {
            return Err("tag must be a tag ID".to_owned());
        }
        let kind = query.kind.filter(|value| !value.is_empty());
        if kind
            .as_ref()
            .is_some_and(|value| !matches!(value.as_str(), "part" | "assembly" | "project"))
        {
            return Err("kind must be part, assembly, or project".to_owned());
        }
        let sort = match query.sort.as_deref() {
            None | Some("" | "updated") => ModelSort::Updated,
            Some("name") => ModelSort::Name,
            Some("created") => ModelSort::Created,
            Some(_) => return Err("sort must be updated, name, or created".to_owned()),
        };
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
        let offset = query.offset.unwrap_or(0);
        if !(1..=MAX_LIMIT).contains(&limit) || offset < 0 {
            return Err(format!(
                "limit must be 1 to {MAX_LIMIT} and offset non-negative"
            ));
        }
        Ok(Self {
            text,
            tag,
            kind,
            sort,
            limit,
            offset,
        })
    }
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

/// `GET /api/v1/models/search`.
pub(crate) async fn route(
    axum::extract::State(state): axum::extract::State<crate::api::ApiState>,
    axum::extract::Query(query): axum::extract::Query<ModelSearchQuery>,
) -> Result<axum::Json<ModelSearchPage>, crate::api_error::ApiError> {
    let request = ModelSearch::parse(query).map_err(crate::api_error::ApiError::BadRequest)?;
    search(&state.pool, &request)
        .await
        .map(axum::Json)
        .map_err(crate::api_error::ApiError::Database)
}

/// `ILIKE` pattern that matches `text` literally anywhere.
fn contains_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

/// Shared filter on `model` and `author`; binds `$1` text, `$2` tag, `$3` kind.
const FILTER: &str = "model.active \
     AND ($1::text IS NULL OR model.name ILIKE $1 OR model.slug ILIKE $1 \
      OR model.description ILIKE $1 OR author.name ILIKE $1 \
      OR EXISTS (SELECT 1 FROM volund.model_tags model_tag JOIN volund.tags tag \
       ON tag.id=model_tag.tag_id WHERE model_tag.model_id=model.id AND tag.name ILIKE $1)) \
     AND ($2::text IS NULL OR EXISTS (SELECT 1 FROM volund.model_tags model_tag \
      JOIN volund.tags tag ON tag.id=model_tag.tag_id \
      WHERE model_tag.model_id=model.id AND tag.public_id::text=$2)) \
     AND ($3::text IS NULL OR model.kind=$3)";

/// Run one search page and the totals of the whole result set.
///
/// # Errors
/// Returns a database diagnostic when the catalog cannot be queried.
pub async fn search(pool: &PgPool, search: &ModelSearch) -> Result<ModelSearchPage, String> {
    let pattern = search.text.as_deref().map(contains_pattern);
    let query = format!(
        "{MODEL_SELECT} WHERE {FILTER} GROUP BY model.id,author.name ORDER BY {} LIMIT $4 OFFSET $5",
        search.sort.order_by()
    );
    let rows = sqlx::query(&query)
        .bind(&pattern)
        .bind(&search.tag)
        .bind(&search.kind)
        .bind(search.limit)
        .bind(search.offset)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("cannot search models: {error}"))?;
    let (total, file_link_total): (i64, i64) = sqlx::query_as(&format!(
        "SELECT count(*), COALESCE(sum((SELECT count(*) FROM volund.model_source_files link \
          WHERE link.model_id=model.id)),0)::bigint \
         FROM volund.models model LEFT JOIN volund.authors author ON author.id=model.author_id \
         WHERE {FILTER}"
    ))
    .bind(&pattern)
    .bind(&search.tag)
    .bind(&search.kind)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count models: {error}"))?;
    Ok(ModelSearchPage {
        items: rows.iter().map(model_from_row).collect(),
        limit: search.limit,
        offset: search.offset,
        total,
        file_link_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_parameters_are_validated_and_defaulted() {
        let search = ModelSearch::parse(ModelSearchQuery {
            q: Some("  bracket ".into()),
            ..ModelSearchQuery::default()
        })
        .unwrap();
        assert_eq!(search.text.as_deref(), Some("bracket"));
        assert_eq!(
            (search.sort, search.limit, search.offset),
            (ModelSort::Updated, DEFAULT_LIMIT, 0)
        );
        let blank = ModelSearch::parse(ModelSearchQuery {
            q: Some("   ".into()),
            ..ModelSearchQuery::default()
        });
        assert_eq!(blank.unwrap().text, None);
        for invalid in [
            ModelSearchQuery {
                sort: Some("random".into()),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                kind: Some("sculpture".into()),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                tag: Some("not-a-uuid".into()),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                limit: Some(0),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                limit: Some(MAX_LIMIT + 1),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                offset: Some(-1),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                q: Some("x".repeat(MAX_QUERY_CHARS + 1)),
                ..ModelSearchQuery::default()
            },
            ModelSearchQuery {
                q: Some("line\nbreak".into()),
                ..ModelSearchQuery::default()
            },
        ] {
            assert!(ModelSearch::parse(invalid).is_err());
        }
    }

    #[test]
    fn search_text_matches_like_wildcards_literally() {
        assert_eq!(contains_pattern("50%_off\\"), "%50\\%\\_off\\\\%");
    }
}
