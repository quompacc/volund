use serde_json::{Value, json};
use sqlx::PgPool;

use crate::settings::{Definition, SettingsError};

#[derive(Debug, Clone, Copy)]
pub struct ImportPolicy {
    pub retention_days: i64,
    pub incoming_capacity_bytes: i64,
    pub max_concurrent_uploads: i64,
}

pub(crate) fn definitions() -> Vec<Definition> {
    vec![
        definition(
            "imports.draftRetentionDays",
            json!(14),
            json!({"minimum": 1, "maximum": 90, "unit": "days"}),
            validate_retention,
        ),
        definition(
            "imports.incomingCapacityBytes",
            json!(20_i64 * 1024 * 1024 * 1024),
            json!({"minimum": 1_073_741_824_i64, "maximum": 1_099_511_627_776_i64, "unit": "bytes"}),
            validate_capacity,
        ),
        definition(
            "imports.maxConcurrentUploads",
            json!(4),
            json!({"minimum": 1, "maximum": 8, "unit": "uploads"}),
            validate_concurrency,
        ),
    ]
}

/// Resolve owner-managed import lifecycle boundaries.
///
/// # Errors
/// Returns an error for invalid persisted settings.
pub async fn load(pool: &PgPool) -> Result<ImportPolicy, SettingsError> {
    let settings = crate::settings::list(pool).await?;
    let integer = |key: &str| {
        settings
            .iter()
            .find(|setting| setting.key == key)
            .and_then(|setting| setting.value.as_i64())
            .ok_or_else(|| SettingsError::BadRequest(format!("missing integer setting: {key}")))
    };
    Ok(ImportPolicy {
        retention_days: integer("imports.draftRetentionDays")?,
        incoming_capacity_bytes: integer("imports.incomingCapacityBytes")?,
        max_concurrent_uploads: integer("imports.maxConcurrentUploads")?,
    })
}

fn definition(
    key: &'static str,
    default: Value,
    constraints: Value,
    validate: fn(&Value) -> Result<(), SettingsError>,
) -> Definition {
    Definition {
        key,
        domain: "imports",
        value_type: "integer",
        default,
        constraints,
        validate,
        environment: None,
        editable: true,
        sensitive: false,
        effect: "immediate",
    }
}

fn validate_retention(value: &Value) -> Result<(), SettingsError> {
    validate_range(value, 1, 90, "import draft retention days")
}

fn validate_capacity(value: &Value) -> Result<(), SettingsError> {
    validate_range(
        value,
        1024 * 1024 * 1024,
        1024_i64.pow(4),
        "incoming capacity",
    )
}

fn validate_concurrency(value: &Value) -> Result<(), SettingsError> {
    validate_range(value, 1, 8, "concurrent import uploads")
}

fn validate_range(value: &Value, min: i64, max: i64, name: &str) -> Result<(), SettingsError> {
    if value
        .as_i64()
        .is_some_and(|value| (min..=max).contains(&value))
    {
        Ok(())
    } else {
        Err(SettingsError::BadRequest(format!(
            "{name} must be an integer between {min} and {max}"
        )))
    }
}
