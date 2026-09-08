use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::catalog_audit;
use crate::session::AuthenticatedSession;

const PLAN_MINUTES: i32 = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub action: String,
    pub target_id: String,
    pub parent_id: Option<String>,
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
pub struct ApplyRequest {
    pub confirmation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub parent_id: Option<String>,
    pub expected_revision: i64,
    pub confirmation: String,
    pub impact: Value,
    pub expires_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySummary {
    pub action: String,
    pub target_id: String,
    pub outcome: &'static str,
    pub revision: i64,
}

#[derive(Debug)]
pub enum LifecycleError {
    BadRequest(String),
    Forbidden,
    NotFound,
    Conflict(String),
    Database(String),
}

impl LifecycleError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "lifecycle_bad_request",
            Self::Forbidden => "lifecycle_forbidden",
            Self::NotFound => "lifecycle_not_found",
            Self::Conflict(_) => "lifecycle_conflict",
            Self::Database(_) => "lifecycle_failure",
        }
    }
}

#[must_use]
pub fn target_type(action: &str) -> &'static str {
    match action {
        "model-file.unlink" => "model-file",
        "model.remove" => "model",
        "source.quarantine" | "source.recover" | "source.purge" => "source-file",
        "author.remove" => "author",
        "tag.remove" => "tag",
        "collection.remove" => "collection",
        _ => "lifecycle-plan",
    }
}

struct Impact {
    target_type: &'static str,
    target_id: String,
    parent_id: Option<String>,
    revision: i64,
    value: Value,
}

struct StoredPlan {
    action: String,
    target_type: String,
    target_id: String,
    parent_id: Option<String>,
    revision: i64,
}

/// Create a short-lived impact preview bound to actor, resource and revision.
///
/// # Errors
/// Returns a classified authorization, validation, stale-resource, or database error.
pub async fn preview(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    request: &PreviewRequest,
) -> Result<PlanSummary, LifecycleError> {
    validate_role(actor, &request.action)?;
    if request.expected_revision <= 0 {
        return Err(LifecycleError::BadRequest(
            "expectedRevision must be positive".to_owned(),
        ));
    }
    let impact = load_impact(pool, request).await?;
    if impact.revision != request.expected_revision {
        return Err(LifecycleError::Conflict(
            "the resource changed; reload its impact before continuing".to_owned(),
        ));
    }
    let confirmation = confirmation(&request.action, &impact.target_id);
    let row = sqlx::query(
        "INSERT INTO volund.lifecycle_plans
         (actor_user_id,action,target_type,target_public_id,parent_public_id,
          expected_revision,confirmation,impact,expires_at)
         VALUES ($1,$2,$3,$4::uuid,$5::uuid,$6,$7,$8,now()+make_interval(mins=>$9))
         RETURNING public_id::text,round(extract(epoch FROM expires_at)*1000)::bigint",
    )
    .bind(actor.database_user_id())
    .bind(&request.action)
    .bind(impact.target_type)
    .bind(&impact.target_id)
    .bind(impact.parent_id.as_deref())
    .bind(impact.revision)
    .bind(&confirmation)
    .bind(&impact.value)
    .bind(PLAN_MINUTES)
    .fetch_one(pool)
    .await
    .map_err(database_error("create lifecycle plan"))?;
    Ok(PlanSummary {
        id: row.get(0),
        action: request.action.clone(),
        target_type: impact.target_type.to_owned(),
        target_id: impact.target_id,
        parent_id: impact.parent_id,
        expected_revision: impact.revision,
        confirmation,
        impact: impact.value,
        expires_at_unix_ms: row.get(1),
    })
}

/// Consume one valid plan and apply a catalog-only lifecycle mutation atomically.
///
/// # Errors
/// Returns a classified error when the plan is invalid, stale, forbidden, or cannot commit.
pub async fn apply_catalog(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    plan_id: &str,
    request: &ApplyRequest,
) -> Result<ApplySummary, LifecycleError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(database_error("begin lifecycle"))?;
    let plan = lock_plan(&mut tx, actor, plan_id, &request.confirmation).await?;
    validate_role(actor, &plan.action)?;
    let revision = match plan.action.as_str() {
        "model-file.unlink" => unlink(&mut tx, &plan).await?,
        "model.remove" => remove_model(&mut tx, &plan).await?,
        "author.remove" => remove_author(&mut tx, &plan).await?,
        "tag.remove" => remove_tag(&mut tx, &plan).await?,
        "collection.remove" => remove_collection(&mut tx, &plan).await?,
        "source.quarantine" | "source.recover" | "source.purge" => {
            return Err(LifecycleError::BadRequest(
                "source lifecycle plans require the filesystem executor".to_owned(),
            ));
        }
        _ => {
            return Err(LifecycleError::BadRequest(
                "unknown lifecycle action".to_owned(),
            ));
        }
    };
    consume_and_audit(&mut tx, actor, plan_id, &plan, revision).await?;
    tx.commit()
        .await
        .map_err(database_error("commit lifecycle"))?;
    Ok(ApplySummary {
        action: plan.action,
        target_id: plan.target_id,
        outcome: "applied",
        revision,
    })
}

fn validate_role(actor: &AuthenticatedSession, action: &str) -> Result<(), LifecycleError> {
    let allowed = match action {
        "model-file.unlink" | "model.remove" => {
            matches!(actor.role.as_str(), "editor" | "administrator" | "owner")
        }
        "source.quarantine" | "source.recover" | "author.remove" | "tag.remove"
        | "collection.remove" => matches!(actor.role.as_str(), "administrator" | "owner"),
        "source.purge" => actor.role == "owner",
        _ => {
            return Err(LifecycleError::BadRequest(
                "unknown lifecycle action".to_owned(),
            ));
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(LifecycleError::Forbidden)
    }
}

async fn load_impact(pool: &PgPool, request: &PreviewRequest) -> Result<Impact, LifecycleError> {
    match request.action.as_str() {
        "model-file.unlink" => model_file_impact(pool, request).await,
        "model.remove" => model_impact(pool, request).await,
        "source.quarantine" | "source.recover" | "source.purge" => {
            source_impact(pool, request).await
        }
        "author.remove" => vocabulary_impact(pool, request, "author").await,
        "tag.remove" => vocabulary_impact(pool, request, "tag").await,
        "collection.remove" => vocabulary_impact(pool, request, "collection").await,
        _ => Err(LifecycleError::BadRequest(
            "unknown lifecycle action".to_owned(),
        )),
    }
}

async fn model_file_impact(
    pool: &PgPool,
    request: &PreviewRequest,
) -> Result<Impact, LifecycleError> {
    let parent = request.parent_id.as_deref().ok_or_else(|| {
        LifecycleError::BadRequest("parentId is required for model-file unlink".to_owned())
    })?;
    let row = sqlx::query(
        "SELECT source.public_id::text,model.public_id::text,model.revision,link.is_primary,
         (SELECT count(*) FROM volund.model_source_files WHERE model_id=model.id)::bigint,
         content.byte_size,content.sha256
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id
         JOIN volund.source_files source ON source.id=link.source_file_id
         JOIN volund.content_objects content ON content.id=source.content_object_id
         WHERE model.public_id::text=$1 AND source.public_id::text=$2 AND model.active",
    )
    .bind(parent)
    .bind(&request.target_id)
    .fetch_optional(pool)
    .await
    .map_err(database_error("load unlink impact"))?
    .ok_or(LifecycleError::NotFound)?;
    Ok(Impact {
        target_type: "model-file",
        target_id: row.get(0),
        parent_id: Some(row.get(1)),
        revision: row.get(2),
        value: json!({"primary":row.get::<bool,_>(3),"modelFileCount":row.get::<i64,_>(4),
            "orphanedModel":row.get::<i64,_>(4)==1,"byteSize":row.get::<i64,_>(5),
            "sha256":row.get::<String,_>(6),"originalBytesDeleted":false}),
    })
}

async fn model_impact(pool: &PgPool, request: &PreviewRequest) -> Result<Impact, LifecycleError> {
    let row = sqlx::query(
        "SELECT model.public_id::text,model.revision,model.name,
         count(DISTINCT link.source_file_id)::bigint,count(DISTINCT component.child_model_id)::bigint,
         count(DISTINCT parent.parent_model_id)::bigint
         FROM volund.models model LEFT JOIN volund.model_source_files link ON link.model_id=model.id
         LEFT JOIN volund.model_components component ON component.parent_model_id=model.id
         LEFT JOIN volund.model_components parent ON parent.child_model_id=model.id
         WHERE model.public_id::text=$1 AND model.active GROUP BY model.id",
    )
    .bind(&request.target_id)
    .fetch_optional(pool).await.map_err(database_error("load model impact"))?
    .ok_or(LifecycleError::NotFound)?;
    Ok(Impact {
        target_type: "model",
        target_id: row.get(0),
        parent_id: None,
        revision: row.get(1),
        value: json!({"name":row.get::<String,_>(2),"fileLinks":row.get::<i64,_>(3),
            "childComponents":row.get::<i64,_>(4),"parentComponents":row.get::<i64,_>(5),
            "originalBytesDeleted":false,"historyRetained":true}),
    })
}

async fn source_impact(pool: &PgPool, request: &PreviewRequest) -> Result<Impact, LifecycleError> {
    let row = sqlx::query(
        "SELECT source.public_id::text,source.lifecycle_revision,source.lifecycle_state,
         content.byte_size,content.sha256,count(DISTINCT link.model_id)::bigint,
         count(DISTINCT dependency.id)::bigint,count(DISTINCT model.id)::bigint,
         quarantine.retention_until <= now(),quarantine.revision
         FROM volund.source_files source JOIN volund.content_objects content ON content.id=source.content_object_id
         LEFT JOIN volund.model_source_files link ON link.source_file_id=source.id
         LEFT JOIN volund.file_dependencies dependency ON dependency.source_file_id=source.id OR dependency.resolved_source_file_id=source.id
         LEFT JOIN volund.models model ON model.thumbnail_source_file_id=source.id
         LEFT JOIN volund.source_quarantines quarantine ON quarantine.source_file_id=source.id AND quarantine.state='quarantined'
         WHERE source.public_id::text=$1 GROUP BY source.id,content.id,quarantine.id",
    )
    .bind(&request.target_id).fetch_optional(pool).await.map_err(database_error("load source impact"))?
    .ok_or(LifecycleError::NotFound)?;
    let state: String = row.get(2);
    let allowed = match request.action.as_str() {
        "source.quarantine" => state == "available",
        "source.recover" | "source.purge" => state == "quarantined",
        _ => false,
    };
    if !allowed {
        return Err(LifecycleError::Conflict(
            "source lifecycle state does not allow this action".to_owned(),
        ));
    }
    let revision = row.try_get::<i64, _>(9).unwrap_or_else(|_| row.get(1));
    Ok(Impact {
        target_type: "source-file",
        target_id: row.get(0),
        parent_id: None,
        revision,
        value: json!({"state":state,"byteSize":row.get::<i64,_>(3),"sha256":row.get::<String,_>(4),
            "liveModelLinks":row.get::<i64,_>(5),"dependencies":row.get::<i64,_>(6),
            "selectedThumbnails":row.get::<i64,_>(7),"retentionExpired":row.try_get::<bool,_>(8).unwrap_or(false)}),
    })
}

async fn vocabulary_impact(
    pool: &PgPool,
    request: &PreviewRequest,
    kind: &str,
) -> Result<Impact, LifecycleError> {
    let (query, target_type) = match kind {
        "author" => (
            "SELECT public_id::text,revision,name,(SELECT count(*) FROM volund.models WHERE author_id=authors.id)::bigint FROM volund.authors WHERE public_id::text=$1 AND active",
            "author",
        ),
        "tag" => (
            "SELECT public_id::text,revision,name,(SELECT count(*) FROM volund.model_tags WHERE tag_id=tags.id)::bigint FROM volund.tags WHERE public_id::text=$1 AND active",
            "tag",
        ),
        _ => (
            "SELECT public_id::text,revision,name,(SELECT count(*) FROM volund.collection_models WHERE collection_id=collections.id)::bigint FROM volund.collections WHERE public_id::text=$1 AND active",
            "collection",
        ),
    };
    let row = sqlx::query(query)
        .bind(&request.target_id)
        .fetch_optional(pool)
        .await
        .map_err(database_error("load vocabulary impact"))?
        .ok_or(LifecycleError::NotFound)?;
    Ok(Impact {
        target_type,
        target_id: row.get(0),
        parent_id: None,
        revision: row.get(1),
        value: json!({"name":row.get::<String,_>(2),"modelReferences":row.get::<i64,_>(3)}),
    })
}

async fn lock_plan(
    tx: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    plan_id: &str,
    supplied: &str,
) -> Result<StoredPlan, LifecycleError> {
    let row = sqlx::query(
        "SELECT action,target_type,target_public_id::text,parent_public_id::text,expected_revision,confirmation
         FROM volund.lifecycle_plans WHERE public_id::text=$1 AND actor_user_id=$2
         AND consumed_at IS NULL AND expires_at>now() FOR UPDATE")
        .bind(plan_id).bind(actor.database_user_id()).fetch_optional(&mut **tx).await
        .map_err(database_error("lock lifecycle plan"))?.ok_or(LifecycleError::NotFound)?;
    if supplied != row.get::<String, _>(5) {
        return Err(LifecycleError::BadRequest(
            "exact lifecycle confirmation is required".to_owned(),
        ));
    }
    Ok(StoredPlan {
        action: row.get(0),
        target_type: row.get(1),
        target_id: row.get(2),
        parent_id: row.get(3),
        revision: row.get(4),
    })
}

async fn unlink(
    tx: &mut Transaction<'_, Postgres>,
    plan: &StoredPlan,
) -> Result<i64, LifecycleError> {
    let parent = plan
        .parent_id
        .as_deref()
        .ok_or_else(|| LifecycleError::BadRequest("unlink plan has no parent".to_owned()))?;
    let model_id:Option<i64>=sqlx::query_scalar("SELECT id FROM volund.models WHERE public_id::text=$1 AND revision=$2 AND active FOR UPDATE")
        .bind(parent).bind(plan.revision).fetch_optional(&mut **tx).await.map_err(database_error("lock unlink model"))?;
    let model_id = model_id.ok_or_else(|| {
        LifecycleError::Conflict("the model changed after impact preview".to_owned())
    })?;
    let removed=sqlx::query("DELETE FROM volund.model_source_files USING volund.source_files source WHERE model_source_files.model_id=$1 AND model_source_files.source_file_id=source.id AND source.public_id::text=$2")
        .bind(model_id).bind(&plan.target_id).execute(&mut **tx).await.map_err(database_error("unlink model file"))?.rows_affected();
    if removed != 1 {
        return Err(LifecycleError::NotFound);
    }
    sqlx::query_scalar("UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision")
        .bind(model_id).fetch_one(&mut **tx).await.map_err(database_error("revise unlinked model"))
}

async fn remove_model(
    tx: &mut Transaction<'_, Postgres>,
    plan: &StoredPlan,
) -> Result<i64, LifecycleError> {
    sqlx::query_scalar("UPDATE volund.models SET active=false,removed_at=now(),revision=revision+1,updated_at=now() WHERE public_id::text=$1 AND revision=$2 AND active RETURNING revision")
        .bind(&plan.target_id).bind(plan.revision).fetch_optional(&mut **tx).await.map_err(database_error("remove model"))?
        .ok_or_else(||LifecycleError::Conflict("the model changed after impact preview".to_owned()))
}

async fn remove_author(
    tx: &mut Transaction<'_, Postgres>,
    plan: &StoredPlan,
) -> Result<i64, LifecycleError> {
    let id:Option<i64>=sqlx::query_scalar("SELECT id FROM volund.authors WHERE public_id::text=$1 AND revision=$2 AND active FOR UPDATE")
        .bind(&plan.target_id).bind(plan.revision).fetch_optional(&mut **tx).await.map_err(database_error("lock author"))?;
    let id = id.ok_or_else(|| {
        LifecycleError::Conflict("the author changed after impact preview".to_owned())
    })?;
    sqlx::query("UPDATE volund.models SET author_id=NULL,revision=revision+1,updated_at=now() WHERE author_id=$1").bind(id).execute(&mut **tx).await.map_err(database_error("clear author references"))?;
    sqlx::query_scalar("UPDATE volund.authors SET active=false,removed_at=now(),revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision")
        .bind(id).fetch_one(&mut **tx).await.map_err(database_error("remove author"))
}

async fn remove_tag(
    tx: &mut Transaction<'_, Postgres>,
    plan: &StoredPlan,
) -> Result<i64, LifecycleError> {
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM volund.tags WHERE public_id::text=$1 AND revision=$2 AND active FOR UPDATE",
    )
    .bind(&plan.target_id)
    .bind(plan.revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error("lock tag"))?;
    let id = id.ok_or_else(|| {
        LifecycleError::Conflict("the tag changed after impact preview".to_owned())
    })?;
    sqlx::query("DELETE FROM volund.model_tags WHERE tag_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(database_error("clear tag references"))?;
    sqlx::query_scalar("UPDATE volund.tags SET active=false,removed_at=now(),revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision")
        .bind(id).fetch_one(&mut **tx).await.map_err(database_error("remove tag"))
}

async fn remove_collection(
    tx: &mut Transaction<'_, Postgres>,
    plan: &StoredPlan,
) -> Result<i64, LifecycleError> {
    let id:Option<i64>=sqlx::query_scalar("SELECT id FROM volund.collections WHERE public_id::text=$1 AND revision=$2 AND active FOR UPDATE")
        .bind(&plan.target_id).bind(plan.revision).fetch_optional(&mut **tx).await.map_err(database_error("lock collection"))?;
    let id = id.ok_or_else(|| {
        LifecycleError::Conflict("the collection changed after impact preview".to_owned())
    })?;
    sqlx::query("DELETE FROM volund.collection_models WHERE collection_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(database_error("clear collection references"))?;
    sqlx::query_scalar("UPDATE volund.collections SET active=false,removed_at=now(),revision=revision+1,updated_at=now() WHERE id=$1 RETURNING revision")
        .bind(id).fetch_one(&mut **tx).await.map_err(database_error("remove collection"))
}

async fn consume_and_audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &AuthenticatedSession,
    plan_id: &str,
    plan: &StoredPlan,
    revision: i64,
) -> Result<(), LifecycleError> {
    sqlx::query("UPDATE volund.lifecycle_plans SET consumed_at=now() WHERE public_id::text=$1")
        .bind(plan_id)
        .execute(&mut **tx)
        .await
        .map_err(database_error("consume lifecycle plan"))?;
    catalog_audit::record(
        tx,
        actor,
        &plan.action,
        &plan.target_type,
        &plan.target_id,
        json!({"revision":revision,"planId":plan_id}),
    )
    .await
    .map_err(|e| LifecycleError::Database(format!("cannot audit lifecycle action: {e}")))
}

fn confirmation(action: &str, target_id: &str) -> String {
    format!("{} {target_id}", action.to_ascii_uppercase())
}
fn database_error(context: &'static str) -> impl Fn(sqlx::Error) -> LifecycleError {
    move |error| LifecycleError::Database(format!("cannot {context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::confirmation;
    #[test]
    fn confirmations_bind_action_and_resource() {
        assert_eq!(confirmation("model.remove", "abc"), "MODEL.REMOVE abc");
    }
}
