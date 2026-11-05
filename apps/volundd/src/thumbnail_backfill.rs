//! Automatic thumbnails for models that still show the neutral placeholder
//! (ADR 0040). The scheduler queues a native conversion of each such model's
//! best 3D source; the converter renders `thumbnail-raster` as a by-product and
//! the catalog shows it as a generated image. A deliberate user choice
//! (`thumbnail_kind` other than `default`) always wins and is never touched.

use sqlx::PgPool;

/// Source formats the native converter can mesh and therefore render.
pub const RENDERABLE_EXTENSIONS: &[&str] = &[
    "step", "stp", "iges", "igs", "brep", "stl", "3mf", "obj", "ply", "gltf", "glb",
];

/// Native conversions the backfill keeps queued or running at the same time.
/// User-requested conversions count against the limit, so interactive previews
/// are never stuck behind a long backfill queue.
pub const MAX_ACTIVE_CONVERSIONS: i64 = 2;

// One candidate per model: the primary file first, then master CAD, CAD,
// printable meshes. A model is skipped while any of its files converts, once
// any file has a thumbnail from this converter version, and files whose
// conversion with this version failed are never retried automatically.
const CANDIDATES: &str = "SELECT file_id FROM ( \
     SELECT DISTINCT ON (model.id) model.id AS model_id, model.updated_at, \
       source.public_id::text AS file_id \
     FROM volund.models model \
     JOIN volund.model_source_files link ON link.model_id = model.id \
     JOIN volund.source_files source ON source.id = link.source_file_id \
     WHERE model.active AND model.thumbnail_kind = 'default' \
       AND source.missing_at IS NULL AND source.lifecycle_state = 'available' \
       AND lower(substring(source.relative_path FROM '[.]([^./]+)$')) = ANY($2) \
       AND NOT EXISTS (SELECT 1 FROM volund.conversion_runs run \
         WHERE run.content_object_id = source.content_object_id \
           AND run.converter_name = 'volund-cad-convert' AND run.converter_version = $1) \
       AND NOT EXISTS (SELECT 1 FROM volund.model_source_files other_link \
         JOIN volund.source_files other ON other.id = other_link.source_file_id \
         JOIN volund.conversion_runs run ON run.content_object_id = other.content_object_id \
         WHERE other_link.model_id = model.id AND (run.status IN ('queued', 'running') \
           OR (run.status = 'ready' AND run.converter_version = $1 AND EXISTS ( \
             SELECT 1 FROM volund.derived_artifacts artifact \
             WHERE artifact.conversion_run_id = run.id \
               AND artifact.artifact_kind = 'thumbnail-raster')))) \
     ORDER BY model.id, link.is_primary DESC, \
       CASE link.role WHEN 'master-cad' THEN 0 WHEN 'cad' THEN 1 \
         WHEN 'printable-mesh' THEN 2 ELSE 3 END, \
       link.ordinal, source.id \
   ) candidate ORDER BY updated_at DESC, model_id LIMIT $3";

/// Queue native conversions for models without a chosen or generated thumbnail.
///
/// Returns the number of newly queued conversions.
///
/// # Errors
/// Returns a database, settings, or enqueue failure.
pub async fn enqueue_missing(pool: &PgPool, max_active: i64) -> Result<u64, String> {
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.conversion_runs WHERE status IN ('queued', 'running')",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count active conversions: {error}"))?;
    let free = max_active - active;
    if free <= 0 {
        return Ok(0);
    }
    let extensions: Vec<String> = RENDERABLE_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_owned())
        .collect();
    let files: Vec<String> = sqlx::query_scalar(CANDIDATES)
        .bind(env!("CARGO_PKG_VERSION"))
        .bind(&extensions)
        .bind(free)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("cannot select thumbnail candidates: {error}"))?;
    if files.is_empty() {
        return Ok(0);
    }
    let profile = default_profile(pool).await?;
    let mut queued = 0;
    for file in files {
        crate::preview_pipeline::enqueue(pool, &file, &profile).await?;
        queued += 1;
    }
    Ok(queued)
}

async fn default_profile(pool: &PgPool) -> Result<String, String> {
    let settings = crate::settings::list(pool)
        .await
        .map_err(|error| format!("cannot read preview settings: {error:?}"))?;
    Ok(settings
        .iter()
        .find(|setting| setting.key == "previews.defaultProfile")
        .and_then(|setting| setting.value.as_str())
        .unwrap_or("web")
        .to_owned())
}
