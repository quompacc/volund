#![allow(clippy::missing_errors_doc)]
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub name: String,
    pub native_preset: String,
    pub linear_deflection: Option<f64>,
    pub angular_deflection: Option<f64>,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionProfile {
    pub id: String,
    pub name: String,
    pub native_preset: String,
    pub linear_deflection: Option<f64>,
    pub angular_deflection: Option<f64>,
    pub enabled: bool,
    pub built_in: bool,
    pub revision: i64,
}

pub struct ResolvedProfile {
    pub database_id: i64,
    pub native_preset: String,
    pub revision: i64,
    pub snapshot: serde_json::Value,
}

/// List profiles. # Errors Returns a sanitized persistence diagnostic.
pub async fn list(pool: &PgPool) -> Result<Vec<ConversionProfile>, String> {
    sqlx::query(PROFILE_SELECT)
        .fetch_all(pool)
        .await
        .map(|rows| rows.iter().map(profile).collect())
        .map_err(|error| format!("cannot list conversion profiles: {error}"))
}

/// Create a validated profile. # Errors Returns validation or persistence failure.
pub async fn create(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    input: &ProfileInput,
    confirmation: &str,
) -> Result<ConversionProfile, String> {
    if confirmation != "APPLY PROFILE new" {
        crate::security_audit::denied(
            pool,
            actor,
            "conversion-profile.create",
            "conversion-profile",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact profile application confirmation is required".to_owned());
    }
    validate(input)?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO volund.conversion_profiles (name,native_preset,linear_deflection,angular_deflection,enabled,updated_by_user_id) VALUES ($1,$2,$3,$4,$5,$6) RETURNING public_id::text")
        .bind(&input.name).bind(&input.native_preset).bind(input.linear_deflection)
        .bind(input.angular_deflection).bind(input.enabled).bind(actor.database_user_id())
        .fetch_one(&mut *tx).await.map_err(|error| format!("cannot create conversion profile: {error}"))?;
    audit(&mut tx, actor, "conversion-profile.create", &id).await?;
    tx.commit().await.map_err(|error| error.to_string())?;
    load(pool, &id).await
}

/// Update with revision control. # Errors Returns validation, conflict, or persistence failure.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    expected_revision: i64,
    input: &ProfileInput,
    confirmation: &str,
) -> Result<ConversionProfile, String> {
    if confirmation != format!("APPLY PROFILE {id}") {
        crate::security_audit::denied(
            pool,
            actor,
            "conversion-profile.update",
            "conversion-profile",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact profile application confirmation is required".to_owned());
    }
    validate(input)?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let updated = sqlx::query_scalar::<_, String>(
        "UPDATE volund.conversion_profiles SET name=$2,native_preset=$3,linear_deflection=$4,angular_deflection=$5,enabled=$6,revision=revision+1,updated_by_user_id=$7,updated_at=now() WHERE public_id::text=$1 AND revision=$8 AND NOT built_in RETURNING public_id::text")
        .bind(id).bind(&input.name).bind(&input.native_preset).bind(input.linear_deflection)
        .bind(input.angular_deflection).bind(input.enabled).bind(actor.database_user_id()).bind(expected_revision)
        .fetch_optional(&mut *tx).await.map_err(|error| format!("cannot update conversion profile: {error}"))?
        .ok_or_else(|| "conversion profile not found or revision changed".to_owned())?;
    audit(&mut tx, actor, "conversion-profile.update", &updated).await?;
    tx.commit().await.map_err(|error| error.to_string())?;
    load(pool, id).await
}

/// Retire without rewriting jobs. # Errors Returns confirmation or persistence failure.
pub async fn retire(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    id: &str,
    expected_revision: i64,
    confirmation: &str,
) -> Result<ConversionProfile, String> {
    if confirmation != format!("RETIRE {id}") {
        crate::security_audit::denied(
            pool,
            actor,
            "conversion-profile.retire",
            "conversion-profile",
            "confirmation_mismatch",
        )
        .await;
        return Err("exact profile retirement confirmation is required".into());
    }
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let updated = sqlx::query_scalar::<_, String>(
        "UPDATE volund.conversion_profiles SET enabled=false,revision=revision+1,updated_by_user_id=$2,updated_at=now() WHERE public_id::text=$1 AND revision=$3 AND enabled AND NOT built_in RETURNING public_id::text")
        .bind(id).bind(actor.database_user_id()).bind(expected_revision).fetch_optional(&mut *tx).await
        .map_err(|error| format!("cannot retire conversion profile: {error}"))?
        .ok_or_else(|| "conversion profile not found, revision changed, or already retired; reload before confirming".to_owned())?;
    audit(&mut tx, actor, "conversion-profile.retire", &updated).await?;
    tx.commit().await.map_err(|error| error.to_string())?;
    load(pool, id).await
}

/// Resolve an enabled snapshot. # Errors Returns not-found or persistence failure.
pub async fn resolve(pool: &PgPool, name_or_id: &str) -> Result<ResolvedProfile, String> {
    let row = sqlx::query("SELECT id,native_preset,revision,linear_deflection,angular_deflection FROM volund.conversion_profiles WHERE (name=$1 OR public_id::text=$1) AND enabled")
        .bind(name_or_id).fetch_optional(pool).await.map_err(|error| format!("cannot resolve conversion profile: {error}"))?
        .ok_or_else(|| "enabled conversion profile not found".to_owned())?;
    let native_preset: String = row.get(1);
    let linear: Option<f64> = row.get(3);
    let angular: Option<f64> = row.get(4);
    Ok(ResolvedProfile {
        database_id: row.get(0),
        native_preset: native_preset.clone(),
        revision: row.get(2),
        snapshot: json!({"nativePreset":native_preset,"linearDeflection":linear,"angularDeflection":angular}),
    })
}

fn validate(input: &ProfileInput) -> Result<(), String> {
    if input.name.trim() != input.name || input.name.is_empty() || input.name.chars().count() > 80 {
        return Err("profile name must contain 1 to 80 trimmed characters".into());
    }
    if !matches!(input.native_preset.as_str(), "web" | "fine") {
        return Err("native preset must be web or fine".into());
    }
    if input
        .linear_deflection
        .is_some_and(|value| !(0.000_001..=1000.0).contains(&value))
    {
        return Err("linear deflection must be between 0.000001 and 1000".into());
    }
    if input
        .angular_deflection
        .is_some_and(|value| !(0.01..=std::f64::consts::PI).contains(&value))
    {
        return Err("angular deflection must be between 0.01 and pi".into());
    }
    Ok(())
}

async fn load(pool: &PgPool, id: &str) -> Result<ConversionProfile, String> {
    sqlx::query(&format!("{PROFILE_SELECT} WHERE public_id::text=$1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("cannot load conversion profile: {error}"))?
        .as_ref()
        .map(profile)
        .ok_or_else(|| "conversion profile not found".to_owned())
}

fn profile(row: &sqlx::postgres::PgRow) -> ConversionProfile {
    ConversionProfile {
        id: row.get(0),
        name: row.get(1),
        native_preset: row.get(2),
        linear_deflection: row.get(3),
        angular_deflection: row.get(4),
        enabled: row.get(5),
        built_in: row.get(6),
        revision: row.get(7),
    }
}

async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    action: &str,
    id: &str,
) -> Result<(), String> {
    sqlx::query("INSERT INTO volund.security_audit_events (actor_user_id,actor_public_id,actor_display_name,action,outcome,target_type,target_public_id) VALUES ($1,$2::uuid,$3,$4,'success','conversion-profile',$5::uuid)")
        .bind(actor.database_user_id()).bind(&actor.user_id).bind(&actor.display_name).bind(action).bind(id)
        .execute(&mut **tx).await.map(|_| ()).map_err(|error| format!("cannot audit conversion profile: {error}"))
}

const PROFILE_SELECT: &str = "SELECT public_id::text,name,native_preset,linear_deflection,angular_deflection,enabled,built_in,revision FROM volund.conversion_profiles";
