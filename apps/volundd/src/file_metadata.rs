use serde_json::json;
use sqlx::{PgPool, Row};

use crate::api_models::UpdateModelFileRequest;
use crate::model_maintenance::ModelError;
use crate::session::AuthenticatedSession;

/// Update metadata owned by one model/source relationship without renaming bytes.
///
/// # Errors
/// Returns validation, lookup, stale-revision, or database errors atomically.
pub async fn update(
    pool: &PgPool,
    actor: &AuthenticatedSession,
    model_id: &str,
    file_id: &str,
    request: UpdateModelFileRequest,
) -> Result<(), ModelError> {
    let caption = bounded(&request.caption, 160, "caption")?;
    let description = bounded(&request.description, 4000, "description")?;
    let notes = bounded(&request.notes, 4000, "notes")?;
    let support_hint = bounded(&request.support_hint, 1000, "supportHint")?;
    if request.expected_revision < 1 {
        return Err(ModelError::BadRequest(
            "expectedRevision must be positive".to_owned(),
        ));
    }
    if request
        .up_axis
        .as_deref()
        .is_some_and(|axis| !matches!(axis, "x" | "y" | "z"))
    {
        return Err(ModelError::BadRequest(
            "upAxis must be x, y, or z".to_owned(),
        ));
    }
    if request
        .orientation
        .iter()
        .any(|value| !value.is_finite() || !(-360.0..=360.0).contains(value))
    {
        return Err(ModelError::BadRequest(
            "orientation values must be finite and between -360 and 360".to_owned(),
        ));
    }
    let mut transaction = pool.begin().await.map_err(ModelError::database)?;
    let row = sqlx::query(
        "SELECT model.id,link.source_file_id,link.revision,source.relative_path \
         FROM volund.models model JOIN volund.model_source_files link ON link.model_id=model.id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         WHERE model.public_id::text=$1 AND source.public_id::text=$2 \
         AND model.active AND source.lifecycle_state='available' \
         FOR UPDATE OF model,link,source",
    )
    .bind(model_id)
    .bind(file_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ModelError::database)?
    .ok_or_else(|| ModelError::NotFound("model file relationship not found".to_owned()))?;
    if row.get::<i64, _>(2) != request.expected_revision {
        return Err(ModelError::RevisionConflict);
    }
    let path: String = row.get(3);
    if request.printable && !is_printable_path(&path) {
        return Err(ModelError::BadRequest(
            "only STL and 3MF files can be marked printable".to_owned(),
        ));
    }
    let revision: i64 = sqlx::query_scalar(
        "UPDATE volund.model_source_files SET caption=$3,description=$4,notes=$5,printable=$6, \
         printed=$7,pre_supported=$8,up_axis=$9,support_hint=$10,orientation_x=$11, \
         orientation_y=$12,orientation_z=$13,revision=revision+1,metadata_updated_at=now() \
         WHERE model_id=$1 AND source_file_id=$2 RETURNING revision",
    )
    .bind(row.get::<i64, _>(0))
    .bind(row.get::<i64, _>(1))
    .bind(caption)
    .bind(description)
    .bind(notes)
    .bind(request.printable)
    .bind(request.printed)
    .bind(request.pre_supported)
    .bind(&request.up_axis)
    .bind(support_hint)
    .bind(request.orientation[0])
    .bind(request.orientation[1])
    .bind(request.orientation[2])
    .fetch_one(&mut *transaction)
    .await
    .map_err(ModelError::database)?;
    sqlx::query("UPDATE volund.models SET revision=revision+1,updated_at=now() WHERE id=$1")
        .bind(row.get::<i64, _>(0))
        .execute(&mut *transaction)
        .await
        .map_err(ModelError::database)?;
    crate::catalog_audit::record(
        &mut transaction,
        actor,
        "model.file.metadata.update",
        "source-file",
        file_id,
        json!({"modelId":model_id,"revision":revision,"fields":["caption","description","notes","printable","printed","preSupported","upAxis","supportHint","orientation"]}),
    )
    .await
    .map_err(ModelError::database)?;
    transaction.commit().await.map_err(ModelError::database)
}

fn bounded<'a>(value: &'a str, maximum: usize, field: &str) -> Result<&'a str, ModelError> {
    let value = value.trim();
    if value.chars().count() > maximum {
        return Err(ModelError::BadRequest(format!(
            "{field} exceeds {maximum} characters"
        )));
    }
    Ok(value)
}

fn is_printable_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("stl") || extension.eq_ignore_ascii_case("3mf")
        })
}

#[cfg(test)]
mod tests {
    use super::{bounded, is_printable_path};

    #[test]
    fn printable_classification_and_text_bounds_are_explicit() {
        assert!(is_printable_path("parts/BODY.STL"));
        assert!(is_printable_path("plate.3mf"));
        assert!(!is_printable_path("assembly.step"));
        assert_eq!(bounded(" note ", 10, "notes").unwrap(), "note");
        assert!(bounded("too long", 3, "notes").is_err());
    }
}
