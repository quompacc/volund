use std::{env, net::SocketAddr};

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

use crate::session::AuthenticatedSession;
use crate::session::SessionPolicy;

#[derive(Debug, Eq, PartialEq)]
pub enum SettingsError {
    BadRequest(String),
    Unknown,
    Conflict,
    Database(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub key: &'static str,
    pub domain: &'static str,
    pub value_type: &'static str,
    pub value: Value,
    pub origin: &'static str,
    pub editable: bool,
    pub sensitive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
    pub revision: i64,
    pub effect: &'static str,
    pub constraints: Value,
}

pub(crate) struct Definition {
    pub(crate) key: &'static str,
    pub(crate) domain: &'static str,
    pub(crate) value_type: &'static str,
    pub(crate) default: Value,
    pub(crate) constraints: Value,
    pub(crate) validate: fn(&Value) -> Result<(), SettingsError>,
    pub(crate) environment: Option<&'static str>,
    pub(crate) editable: bool,
    pub(crate) sensitive: bool,
    pub(crate) effect: &'static str,
}

/// Return the complete code-owned registry and its effective values.
///
/// # Errors
///
/// Returns a database error when persisted values cannot be loaded.
pub async fn list(pool: &PgPool) -> Result<Vec<SettingView>, SettingsError> {
    let rows =
        sqlx::query("SELECT setting_key, value_json, revision FROM volund.instance_settings")
            .fetch_all(pool)
            .await
            .map_err(database_error("list instance settings"))?;
    definitions()
        .into_iter()
        .map(|definition| {
            if let Some(variable) = definition.environment {
                let environment_value = env::var(variable).ok();
                return operator_view(definition, environment_value.as_deref());
            }
            let persisted = rows
                .iter()
                .find(|row| row.get::<String, _>(0) == definition.key);
            if let Some(row) = persisted {
                let value: Value = row.get(1);
                (definition.validate)(&value)?;
                Ok(view(definition, value, "persisted", row.get(2)))
            } else {
                let value = definition.default.clone();
                Ok(view(definition, value, "default", 0))
            }
        })
        .collect()
}

/// Resolve the effective session expiry policy from the typed registry.
///
/// # Errors
///
/// Returns a settings error when persisted values cannot be loaded or are
/// inconsistent with the registry.
pub async fn session_policy(pool: &PgPool) -> Result<SessionPolicy, SettingsError> {
    let settings = list(pool).await?;
    let idle_minutes = integer_value(&settings, "security.sessionIdleMinutes")?;
    let absolute_hours = integer_value(&settings, "security.sessionAbsoluteHours")?;
    let idle_minutes = i32::try_from(idle_minutes)
        .map_err(|_| SettingsError::BadRequest("session idle duration is too large".to_owned()))?;
    let absolute_minutes = i32::try_from(absolute_hours * 60).map_err(|_| {
        SettingsError::BadRequest("absolute session duration is too large".to_owned())
    })?;
    Ok(SessionPolicy::from_minutes(idle_minutes, absolute_minutes))
}

fn integer_value(settings: &[SettingView], key: &str) -> Result<i64, SettingsError> {
    settings
        .iter()
        .find(|setting| setting.key == key)
        .and_then(|setting| setting.value.as_i64())
        .ok_or_else(|| SettingsError::BadRequest(format!("missing integer setting: {key}")))
}

/// Validate and persist one known setting with optimistic revision control.
///
/// # Errors
///
/// Returns validation, unknown-key, revision-conflict, or persistence errors.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    key: &str,
    value: Value,
    expected_revision: i64,
    confirmation: Option<&str>,
) -> Result<SettingView, SettingsError> {
    let definition = definitions()
        .into_iter()
        .find(|definition| definition.key == key)
        .ok_or(SettingsError::Unknown)?;
    if !definition.editable {
        return Err(SettingsError::BadRequest(
            "read-only settings cannot be changed through the API".to_owned(),
        ));
    }
    if (matches!(definition.domain, "jobs" | "retention")
        || key.starts_with("imports.draftRetention")
        || key.starts_with("imports.incomingCapacity")
        || key.starts_with("imports.maxConcurrent"))
        && confirmation != Some(&format!("APPLY POLICY {key}"))
    {
        crate::security_audit::denied(
            pool,
            actor,
            "setting.update",
            "setting",
            "confirmation_mismatch",
        )
        .await;
        return Err(SettingsError::BadRequest(
            "exact policy confirmation is required".to_owned(),
        ));
    }
    (definition.validate)(&value)?;
    if expected_revision < 0 {
        return Err(SettingsError::BadRequest(
            "expected revision must not be negative".to_owned(),
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(database_error("begin setting update"))?;
    let current = sqlx::query(
        "SELECT revision FROM volund.instance_settings WHERE setting_key = $1 FOR UPDATE",
    )
    .bind(key)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error("lock instance setting"))?;
    let current_revision = current.as_ref().map_or(0, |row| row.get(0));
    if current_revision != expected_revision {
        return Err(SettingsError::Conflict);
    }
    let revision = persist(&mut transaction, actor, key, &value, current.is_some()).await?;
    sqlx::query(
        "INSERT INTO volund.security_audit_events \
         (actor_user_id, actor_public_id, actor_display_name, action, outcome, target_type, metadata) \
         VALUES ($1, $2::uuid, $3, 'setting.update', 'success', 'setting', \
         jsonb_build_object('key', $4::text, 'revision', $5::bigint))",
    )
    .bind(actor.database_user_id())
    .bind(&actor.user_id)
    .bind(&actor.display_name)
    .bind(key)
    .bind(revision)
    .execute(&mut *transaction)
    .await
    .map_err(database_error("audit setting update"))?;
    transaction
        .commit()
        .await
        .map_err(database_error("commit setting update"))?;
    Ok(view(definition, value, "persisted", revision))
}

async fn persist(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &AuthenticatedSession,
    key: &str,
    value: &Value,
    exists: bool,
) -> Result<i64, SettingsError> {
    if exists {
        sqlx::query_scalar(
            "UPDATE volund.instance_settings SET value_json = $2, revision = revision + 1, \
             updated_by_user_id = $3, updated_at = now() WHERE setting_key = $1 RETURNING revision",
        )
        .bind(key)
        .bind(value)
        .bind(actor.database_user_id())
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error("update instance setting"))
    } else {
        sqlx::query_scalar(
            "INSERT INTO volund.instance_settings \
             (setting_key, value_json, revision, updated_by_user_id) \
             VALUES ($1, $2, 1, $3) RETURNING revision",
        )
        .bind(key)
        .bind(value)
        .bind(actor.database_user_id())
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error("create instance setting"))
    }
}

fn editable_definitions() -> Vec<Definition> {
    vec![
        editable(
            "instance.name",
            "instance",
            "string",
            json!("VÖLUND"),
            json!({"minLength": 1, "maxLength": 80}),
            validate_instance_name,
        ),
        editable(
            "instance.locale",
            "instance",
            "enum",
            json!("de-DE"),
            json!({"allowed": ["de-DE", "en-US"]}),
            validate_locale,
        ),
        editable(
            "instance.timeZone",
            "instance",
            "enum",
            json!("Europe/Berlin"),
            json!({"allowed": ["Europe/Berlin", "UTC", "Europe/London", "America/New_York", "America/Los_Angeles", "Asia/Tokyo"]}),
            validate_time_zone,
        ),
        editable(
            "catalog.defaultView",
            "catalog",
            "enum",
            json!("models"),
            json!({"allowed": ["models", "files"]}),
            validate_catalog_view,
        ),
        editable(
            "security.sessionIdleMinutes",
            "security",
            "integer",
            json!(480),
            json!({"minimum": 5, "maximum": 1440, "unit": "minutes"}),
            validate_session_idle,
        ),
        editable(
            "security.sessionAbsoluteHours",
            "security",
            "integer",
            json!(720),
            json!({"minimum": 1, "maximum": 2160, "unit": "hours"}),
            validate_session_absolute,
        ),
        editable(
            "imports.defaultModelKind",
            "imports",
            "enum",
            json!("assembly"),
            json!({"allowed": ["assembly", "project", "part"]}),
            validate_import_kind,
        ),
        editable(
            "previews.defaultProfile",
            "previews",
            "enum",
            json!("web"),
            json!({"allowed": ["web", "fine"]}),
            validate_preview_profile,
        ),
        editable(
            "thumbnails.defaultSource",
            "thumbnails",
            "enum",
            json!("primary-cad"),
            json!({"allowed": ["primary-cad", "image-first"]}),
            validate_thumbnail_source,
        ),
    ]
}

fn definitions() -> Vec<Definition> {
    let mut registry = editable_definitions();
    registry.extend(crate::import_policy::definitions());
    registry.extend(crate::operational_policy::definitions());
    registry.extend([
        compiled_integer("limits.importMaxFiles", json!(10_000), "files"),
        compiled_integer(
            "limits.importMaxBytes",
            json!(10_i64 * 1024 * 1024 * 1024),
            "bytes",
        ),
        compiled_integer("limits.apiPageSize", json!(200), "items"),
        operator(
            "runtime.listenAddress",
            "runtime",
            "string",
            json!("127.0.0.1:8080"),
            validate_socket_address,
            "VOLUND_LISTEN_ADDR",
            false,
        ),
        operator(
            "security.secureCookies",
            "security",
            "boolean",
            json!(false),
            validate_boolean,
            "VOLUND_SECURE_COOKIES",
            false,
        ),
        operator(
            "database.connectionOverride",
            "database",
            "secret",
            Value::Null,
            validate_secret,
            "VOLUND_DATABASE_URL",
            true,
        ),
        operator(
            "security.bootstrapTokenFile",
            "security",
            "secret",
            Value::Null,
            validate_secret,
            "VOLUND_BOOTSTRAP_TOKEN_FILE",
            true,
        ),
    ]);
    registry
}

fn editable(
    key: &'static str,
    domain: &'static str,
    value_type: &'static str,
    default: Value,
    constraints: Value,
    validate: fn(&Value) -> Result<(), SettingsError>,
) -> Definition {
    Definition {
        key,
        domain,
        value_type,
        default,
        constraints,
        validate,
        environment: None,
        editable: true,
        sensitive: false,
        effect: "immediate",
    }
}

fn compiled_integer(key: &'static str, default: Value, unit: &'static str) -> Definition {
    Definition {
        key,
        domain: "limits",
        value_type: "integer",
        default,
        constraints: json!({"compiled": true, "unit": unit}),
        validate: validate_positive_integer,
        environment: None,
        editable: false,
        sensitive: false,
        effect: "immediate",
    }
}

fn operator(
    key: &'static str,
    domain: &'static str,
    value_type: &'static str,
    default: Value,
    validate: fn(&Value) -> Result<(), SettingsError>,
    environment: &'static str,
    sensitive: bool,
) -> Definition {
    Definition {
        key,
        domain,
        value_type,
        default,
        constraints: json!({"operatorManaged": true}),
        validate,
        environment: Some(environment),
        editable: false,
        sensitive,
        effect: "restart",
    }
}

fn view(definition: Definition, value: Value, origin: &'static str, revision: i64) -> SettingView {
    SettingView {
        key: definition.key,
        domain: definition.domain,
        value_type: definition.value_type,
        value,
        origin,
        editable: definition.editable,
        sensitive: false,
        configured: None,
        revision,
        effect: definition.effect,
        constraints: definition.constraints,
    }
}

fn operator_view(
    definition: Definition,
    environment_value: Option<&str>,
) -> Result<SettingView, SettingsError> {
    let configured = environment_value.is_some();
    let value = if definition.sensitive {
        Value::Null
    } else if let Some(raw) = environment_value {
        parse_environment_value(definition.value_type, raw)?
    } else {
        definition.default.clone()
    };
    (definition.validate)(&value)?;
    Ok(SettingView {
        key: definition.key,
        domain: definition.domain,
        value_type: definition.value_type,
        value,
        origin: if configured { "environment" } else { "default" },
        editable: false,
        sensitive: definition.sensitive,
        configured: definition.sensitive.then_some(configured),
        revision: 0,
        effect: definition.effect,
        constraints: definition.constraints,
    })
}

fn parse_environment_value(value_type: &str, raw: &str) -> Result<Value, SettingsError> {
    match value_type {
        "string" => Ok(json!(raw)),
        "boolean" => match raw {
            "true" | "1" => Ok(json!(true)),
            "false" | "0" => Ok(json!(false)),
            _ => Err(SettingsError::BadRequest(
                "operator boolean must be true, false, 1, or 0".to_owned(),
            )),
        },
        _ => Err(SettingsError::BadRequest(
            "unsupported operator setting type".to_owned(),
        )),
    }
}

fn validate_instance_name(value: &Value) -> Result<(), SettingsError> {
    let Some(value) = value.as_str() else {
        return Err(SettingsError::BadRequest(
            "instance name must be a string".to_owned(),
        ));
    };
    if value.trim() != value || value.is_empty() || value.chars().count() > 80 {
        return Err(SettingsError::BadRequest(
            "instance name must contain 1 to 80 trimmed characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_locale(value: &Value) -> Result<(), SettingsError> {
    validate_enum(value, &["de-DE", "en-US"], "locale")
}

fn validate_time_zone(value: &Value) -> Result<(), SettingsError> {
    validate_enum(
        value,
        &[
            "Europe/Berlin",
            "UTC",
            "Europe/London",
            "America/New_York",
            "America/Los_Angeles",
            "Asia/Tokyo",
        ],
        "time zone",
    )
}

fn validate_session_idle(value: &Value) -> Result<(), SettingsError> {
    validate_integer_range(value, 5, 1440, "session idle duration")
}

fn validate_session_absolute(value: &Value) -> Result<(), SettingsError> {
    validate_integer_range(value, 1, 2160, "absolute session duration")
}

fn validate_import_kind(value: &Value) -> Result<(), SettingsError> {
    validate_enum(value, &["assembly", "project", "part"], "import model kind")
}

fn validate_preview_profile(value: &Value) -> Result<(), SettingsError> {
    validate_enum(value, &["web", "fine"], "preview profile")
}

fn validate_thumbnail_source(value: &Value) -> Result<(), SettingsError> {
    validate_enum(value, &["primary-cad", "image-first"], "thumbnail source")
}

fn validate_positive_integer(value: &Value) -> Result<(), SettingsError> {
    validate_integer_range(value, 1, i64::MAX, "operational limit")
}

fn validate_integer_range(
    value: &Value,
    minimum: i64,
    maximum: i64,
    name: &str,
) -> Result<(), SettingsError> {
    if value
        .as_i64()
        .is_some_and(|value| (minimum..=maximum).contains(&value))
    {
        Ok(())
    } else {
        Err(SettingsError::BadRequest(format!(
            "{name} must be an integer between {minimum} and {maximum}"
        )))
    }
}

fn validate_catalog_view(value: &Value) -> Result<(), SettingsError> {
    validate_enum(value, &["models", "files"], "catalog default view")
}

fn validate_socket_address(value: &Value) -> Result<(), SettingsError> {
    value
        .as_str()
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .map(|_| ())
        .ok_or_else(|| SettingsError::BadRequest("listen address is invalid".to_owned()))
}

fn validate_boolean(value: &Value) -> Result<(), SettingsError> {
    value
        .as_bool()
        .map(|_| ())
        .ok_or_else(|| SettingsError::BadRequest("setting must be a boolean".to_owned()))
}

fn validate_secret(value: &Value) -> Result<(), SettingsError> {
    if value.is_null() {
        Ok(())
    } else {
        Err(SettingsError::BadRequest(
            "secret values cannot be exposed".to_owned(),
        ))
    }
}

fn validate_enum(value: &Value, allowed: &[&str], name: &str) -> Result<(), SettingsError> {
    if value.as_str().is_some_and(|value| allowed.contains(&value)) {
        Ok(())
    } else {
        Err(SettingsError::BadRequest(format!("{name} is invalid")))
    }
}

fn database_error(context: &'static str) -> impl FnOnce(sqlx::Error) -> SettingsError {
    move |error| SettingsError::Database(format!("{context}: {error}"))
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
