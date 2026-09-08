use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api_models::Page;
use crate::metadata_normalization::normalize_identity;
use crate::session::AuthenticatedSession;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorSummary {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
    pub provenance_source: String,
    pub provenance_note: Option<String>,
    pub active: bool,
    pub merged_into_id: Option<String>,
    pub model_count: i64,
    pub revision: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorInput {
    pub name: String,
    pub website: Option<String>,
    #[serde(default = "unknown_provenance")]
    pub provenance_source: String,
    pub provenance_note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorUpdate {
    pub expected_revision: i64,
    #[serde(flatten)]
    pub input: AuthorInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorMerge {
    pub target_author_id: String,
    pub confirmation: String,
}

#[derive(Debug)]
pub enum AuthorError {
    BadRequest(String),
    NotFound,
    Conflict(String),
    RevisionConflict,
    Database(String),
}

struct ValidAuthor {
    name: String,
    normalized_name: String,
    website: Option<String>,
    provenance_source: String,
    provenance_note: Option<String>,
}

fn unknown_provenance() -> String {
    "unknown".to_owned()
}

/// List authors with a stable bounded page.
///
/// # Errors
/// Returns a database diagnostic when author rows cannot be loaded.
pub async fn list(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    include_merged: bool,
) -> Result<Page<AuthorSummary>, String> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows = sqlx::query(
        "SELECT author.public_id::text,author.name,author.website,author.provenance_source, \
         author.provenance_note,author.active,merged.public_id::text, \
         count(model.id)::bigint,author.revision, \
         round(extract(epoch FROM author.updated_at)*1000)::bigint \
         FROM volund.authors author LEFT JOIN volund.authors merged \
         ON merged.id=author.merged_into_id LEFT JOIN volund.models model \
         ON model.author_id=author.id WHERE ($3 OR author.active) \
         GROUP BY author.id,merged.public_id ORDER BY author.name,author.id LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .bind(include_merged)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list authors: {error}"))?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.authors WHERE $1 OR active")
        .bind(include_merged)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("cannot count authors: {error}"))?;
    Ok(Page {
        items: rows.iter().map(author_from_row).collect(),
        limit,
        offset,
        total,
    })
}

/// Load one author, including a retained merged alias.
///
/// # Errors
/// Returns a database diagnostic when the author cannot be loaded.
pub async fn get(pool: &PgPool, author_id: &str) -> Result<Option<AuthorSummary>, String> {
    let row = sqlx::query(
        "SELECT author.public_id::text,author.name,author.website,author.provenance_source, \
         author.provenance_note,author.active,merged.public_id::text, \
         count(model.id)::bigint,author.revision, \
         round(extract(epoch FROM author.updated_at)*1000)::bigint \
         FROM volund.authors author LEFT JOIN volund.authors merged \
         ON merged.id=author.merged_into_id LEFT JOIN volund.models model \
         ON model.author_id=author.id WHERE author.public_id::text=$1 \
         GROUP BY author.id,merged.public_id",
    )
    .bind(author_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load author: {error}"))?;
    Ok(row.as_ref().map(author_from_row))
}

/// Create a reusable author identity with actor evidence.
///
/// # Errors
/// Returns validation, collision, or database errors without a partial author.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: &AuthorInput,
) -> Result<AuthorSummary, AuthorError> {
    let valid = validate(input)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO volund.authors \
         (name,normalized_name,website,provenance_source,provenance_note) \
         VALUES ($1,$2,$3,$4,$5) RETURNING public_id::text",
    )
    .bind(valid.name)
    .bind(valid.normalized_name)
    .bind(valid.website)
    .bind(valid.provenance_source)
    .bind(valid.provenance_note)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "author.create",
        "author",
        &id,
        json!({"fields":["name","website","provenance"],"revision":1}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, &id)
        .await
        .map_err(AuthorError::Database)?
        .ok_or(AuthorError::NotFound)
}

/// Update a live author using optimistic concurrency.
///
/// # Errors
/// Returns validation, lookup, collision, revision, or database errors atomically.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    author_id: &str,
    request: &AuthorUpdate,
) -> Result<AuthorSummary, AuthorError> {
    let valid = validate(&request.input)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    let current = sqlx::query(
        "SELECT id,revision,active FROM volund.authors WHERE public_id::text=$1 FOR UPDATE",
    )
    .bind(author_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(AuthorError::NotFound)?;
    if !current.get::<bool, _>(2) {
        return Err(AuthorError::Conflict(
            "merged authors cannot be edited".to_owned(),
        ));
    }
    if current.get::<i64, _>(1) != request.expected_revision {
        return Err(AuthorError::RevisionConflict);
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.authors SET name=$2,normalized_name=$3,website=$4, \
         provenance_source=$5,provenance_note=$6,updated_at=now(),revision=revision+1 \
         WHERE id=$1 RETURNING revision",
    )
    .bind(current.get::<i64, _>(0))
    .bind(valid.name)
    .bind(valid.normalized_name)
    .bind(valid.website)
    .bind(valid.provenance_source)
    .bind(valid.provenance_note)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "author.update",
        "author",
        author_id,
        json!({"fields":["name","website","provenance"],"revision":revision}),
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    get(pool, author_id)
        .await
        .map_err(AuthorError::Database)?
        .ok_or(AuthorError::NotFound)
}

/// Merge one author into another while retaining the source as an alias.
///
/// # Errors
/// Returns confirmation, lookup, conflict, or database errors atomically.
pub async fn merge(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    source_id: &str,
    request: &AuthorMerge,
) -> Result<AuthorSummary, AuthorError> {
    if source_id == request.target_author_id {
        return Err(AuthorError::BadRequest(
            "an author cannot be merged into itself".to_owned(),
        ));
    }
    let expected = format!("MERGE AUTHOR {source_id} INTO {}", request.target_author_id);
    if request.confirmation != expected {
        crate::security_audit::denied_target(
            pool,
            actor,
            "author.merge",
            "author",
            source_id,
            "confirmation_mismatch",
        )
        .await;
        return Err(AuthorError::BadRequest(
            "exact author merge confirmation is required".to_owned(),
        ));
    }
    let mut tx = pool.begin().await.map_err(database_error)?;
    let rows = sqlx::query(
        "SELECT id,public_id::text,active FROM volund.authors \
         WHERE public_id::text IN ($1,$2) ORDER BY id FOR UPDATE",
    )
    .bind(source_id)
    .bind(&request.target_author_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    if rows.len() != 2 {
        return Err(AuthorError::NotFound);
    }
    let source = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == source_id)
        .ok_or(AuthorError::NotFound)?;
    let target = rows
        .iter()
        .find(|row| row.get::<String, _>(1) == request.target_author_id)
        .ok_or(AuthorError::NotFound)?;
    if !source.get::<bool, _>(2) || !target.get::<bool, _>(2) {
        return Err(AuthorError::Conflict(
            "only active authors can be merged".to_owned(),
        ));
    }
    let changed_models = sqlx::query(
        "UPDATE volund.models SET author_id=$1,updated_at=now(),revision=revision+1 \
        WHERE author_id=$2 RETURNING public_id::text,revision",
    )
    .bind(target.get::<i64, _>(0))
    .bind(source.get::<i64, _>(0))
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let reassigned = changed_models.len();
    sqlx::query("UPDATE volund.authors SET active=false,merged_into_id=$2,updated_at=now(),revision=revision+1 WHERE id=$1")
        .bind(source.get::<i64, _>(0)).bind(target.get::<i64, _>(0)).execute(&mut *tx).await
        .map_err(database_error)?;
    crate::catalog_audit::record(
        &mut tx,
        actor,
        "author.merge",
        "author",
        source_id,
        json!({"targetAuthorId":request.target_author_id,"modelsReassigned":reassigned}),
    )
    .await
    .map_err(database_error)?;
    for model in changed_models {
        crate::catalog_audit::record(
            &mut tx,
            actor,
            "model.author.merge",
            "model",
            &model.get::<String, _>(0),
            json!({"sourceAuthorId":source_id,"targetAuthorId":request.target_author_id,
                "revision":model.get::<i64, _>(1)}),
        )
        .await
        .map_err(database_error)?;
    }
    tx.commit().await.map_err(database_error)?;
    get(pool, &request.target_author_id)
        .await
        .map_err(AuthorError::Database)?
        .ok_or(AuthorError::NotFound)
}

fn validate(input: &AuthorInput) -> Result<ValidAuthor, AuthorError> {
    let (name, normalized_name) =
        normalize_identity(&input.name, 160).map_err(AuthorError::BadRequest)?;
    let website = input
        .website
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(validate_website)
        .transpose()?;
    if !matches!(
        input.provenance_source.as_str(),
        "unknown" | "import" | "user" | "website"
    ) {
        return Err(AuthorError::BadRequest(
            "invalid provenance source".to_owned(),
        ));
    }
    let provenance_note = input
        .provenance_note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if provenance_note
        .as_ref()
        .is_some_and(|note| note.chars().count() > 2_000)
    {
        return Err(AuthorError::BadRequest(
            "provenance note exceeds 2000 characters".to_owned(),
        ));
    }
    Ok(ValidAuthor {
        name,
        normalized_name,
        website,
        provenance_source: input.provenance_source.clone(),
        provenance_note,
    })
}

fn validate_website(value: &str) -> Result<String, AuthorError> {
    if value.chars().count() > 2_048
        || value.chars().any(char::is_whitespace)
        || !(value.starts_with("https://") || value.starts_with("http://"))
    {
        return Err(AuthorError::BadRequest(
            "author website must be a bounded HTTP or HTTPS URL".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn author_from_row(row: &sqlx::postgres::PgRow) -> AuthorSummary {
    AuthorSummary {
        id: row.get(0),
        name: row.get(1),
        website: row.get(2),
        provenance_source: row.get(3),
        provenance_note: row.get(4),
        active: row.get(5),
        merged_into_id: row.get(6),
        model_count: row.get(7),
        revision: row.get(8),
        updated_at_unix_ms: row.get(9),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn database_error(error: sqlx::Error) -> AuthorError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AuthorError::Conflict(
            "an active author with this normalized name already exists".to_owned(),
        )
    } else {
        AuthorError::Database(format!("cannot persist author: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_websites_and_provenance_are_strictly_bounded() {
        let valid = AuthorInput {
            name: "  VÖLUND\u{2003}Lab ".to_owned(),
            website: Some("https://example.test/source".to_owned()),
            provenance_source: "website".to_owned(),
            provenance_note: Some("Published profile".to_owned()),
        };
        let normalized = validate(&valid).unwrap();
        assert_eq!(normalized.name, "VÖLUND Lab");
        assert_eq!(normalized.normalized_name, "völund lab");
        assert!(
            validate(&AuthorInput {
                website: Some("file:///etc/passwd".to_owned()),
                ..valid
            })
            .is_err()
        );
    }
}
