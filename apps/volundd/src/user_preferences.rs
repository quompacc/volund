use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;
use crate::settings::SettingsError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    pub preview_auto_load: String,
    pub background: String,
    pub grid_visible: bool,
    pub contrast: String,
    pub render_style: String,
    pub problem_minimum_severity: String,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateUserPreferences {
    pub preview_auto_load: String,
    pub background: String,
    pub grid_visible: bool,
    pub contrast: String,
    pub render_style: String,
    pub problem_minimum_severity: String,
    pub expected_revision: i64,
}

/// Load the authenticated user's preferences or bounded defaults.
///
/// # Errors
/// Returns a database diagnostic when the preference row cannot be read.
pub async fn get(
    pool: &PgPool,
    actor: &AuthenticatedSession,
) -> Result<UserPreferences, SettingsError> {
    let row = sqlx::query(
        "SELECT preview_auto_load,background,grid_visible,contrast,render_style,
         problem_minimum_severity,revision FROM volund.user_preferences WHERE user_id=$1",
    )
    .bind(actor.database_user_id())
    .fetch_optional(pool)
    .await
    .map_err(database("load user preferences"))?;
    Ok(row.as_ref().map_or_else(defaults, from_row))
}

/// Validate and atomically persist only the authenticated user's preferences.
///
/// # Errors
/// Returns validation, stale-revision, or database errors.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    request: UpdateUserPreferences,
) -> Result<UserPreferences, SettingsError> {
    validate(&request)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(database("begin preference update"))?;
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM volund.user_preferences WHERE user_id=$1 FOR UPDATE",
    )
    .bind(actor.database_user_id())
    .fetch_optional(&mut *tx)
    .await
    .map_err(database("lock user preferences"))?;
    if current.unwrap_or(0) != request.expected_revision {
        return Err(SettingsError::Conflict);
    }
    let row = sqlx::query(
        "INSERT INTO volund.user_preferences
         (user_id,preview_auto_load,background,grid_visible,contrast,render_style,problem_minimum_severity,revision)
         VALUES ($1,$2,$3,$4,$5,$6,$7,1)
         ON CONFLICT (user_id) DO UPDATE SET preview_auto_load=EXCLUDED.preview_auto_load,
         background=EXCLUDED.background,grid_visible=EXCLUDED.grid_visible,contrast=EXCLUDED.contrast,
         render_style=EXCLUDED.render_style,problem_minimum_severity=EXCLUDED.problem_minimum_severity,
         revision=user_preferences.revision+1,updated_at=now()
         RETURNING preview_auto_load,background,grid_visible,contrast,render_style,problem_minimum_severity,revision",
    )
    .bind(actor.database_user_id())
    .bind(&request.preview_auto_load)
    .bind(&request.background)
    .bind(request.grid_visible)
    .bind(&request.contrast)
    .bind(&request.render_style)
    .bind(&request.problem_minimum_severity)
    .fetch_one(&mut *tx)
    .await
    .map_err(database("persist user preferences"))?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events
         (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id,metadata)
         VALUES ($1,$2::uuid,$3,'preferences.update','success','user',$2::uuid,
         jsonb_build_object('revision',$4::bigint,'fields',jsonb_build_array(
         'previewAutoLoad','background','gridVisible','contrast','renderStyle','problemMinimumSeverity')))",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(row.get::<i64, _>(6))
    .execute(&mut *tx)
    .await
    .map_err(database("audit preference update"))?;
    tx.commit()
        .await
        .map_err(database("commit preference update"))?;
    Ok(from_row(&row))
}

fn validate(request: &UpdateUserPreferences) -> Result<(), SettingsError> {
    if request.expected_revision < 0
        || !["manual", "selected", "visible"].contains(&request.preview_auto_load.as_str())
        || !["dark", "light", "system"].contains(&request.background.as_str())
        || !["balanced", "high"].contains(&request.contrast.as_str())
        || !["solid", "wireframe"].contains(&request.render_style.as_str())
        || !["info", "warning", "error"].contains(&request.problem_minimum_severity.as_str())
    {
        return Err(SettingsError::BadRequest(
            "user preferences contain an unsupported value".to_owned(),
        ));
    }
    Ok(())
}

fn defaults() -> UserPreferences {
    UserPreferences {
        preview_auto_load: "selected".to_owned(),
        background: "dark".to_owned(),
        grid_visible: true,
        contrast: "balanced".to_owned(),
        render_style: "solid".to_owned(),
        problem_minimum_severity: "warning".to_owned(),
        revision: 0,
    }
}

fn from_row(row: &sqlx::postgres::PgRow) -> UserPreferences {
    UserPreferences {
        preview_auto_load: row.get(0),
        background: row.get(1),
        grid_visible: row.get(2),
        contrast: row.get(3),
        render_style: row.get(4),
        problem_minimum_severity: row.get(5),
        revision: row.get(6),
    }
}

fn database(context: &'static str) -> impl FnOnce(sqlx::Error) -> SettingsError {
    move |error| SettingsError::Database(format!("{context}: {error}"))
}
