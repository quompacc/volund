//! Portable per-model exit package (ADR 0029).
//!
//! The archive contains every available original of one model below
//! `originals/<root-key>/<relative-path>`, a `SHA256SUMS` file for tool-free
//! verification and a versioned `manifest.json` with the model metadata,
//! components and per-file metadata. Derived artifacts are deliberately omitted.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::model_catalog;

pub const FORMAT: &str = "volund-model-export-v1";
pub const FORMAT_VERSION: u32 = 1;
const COPY_BUFFER_BYTES: usize = 256 * 1024;

/// One linked source file and where it will be placed in the archive.
#[derive(Debug, Clone)]
pub struct ExportSource {
    pub file_id: String,
    pub archive_path: String,
    /// Resolved original below its library root, or `None` when excluded.
    pub path: Option<PathBuf>,
    pub exclusion_reason: Option<&'static str>,
    pub expected_size: i64,
    pub sha256: String,
}

/// Everything needed to write the archive without touching the database again.
#[derive(Debug, Clone)]
pub struct ModelExportPlan {
    pub model_id: String,
    pub slug: String,
    pub model: Value,
    pub components: Value,
    pub files: Vec<Value>,
    pub sources: Vec<ExportSource>,
}

/// Summary of a written archive.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ArchiveSummary {
    pub bytes: u64,
    pub included_files: usize,
    pub excluded_files: usize,
}

/// Collect metadata and resolve originals for one active model.
///
/// # Errors
///
/// Returns a safe message when the catalog cannot be read.
pub async fn plan(pool: &PgPool, model_id: &str) -> Result<Option<ModelExportPlan>, String> {
    let Some(model) = model_catalog::get_model(pool, model_id).await? else {
        return Ok(None);
    };
    let files = model_catalog::list_model_files(pool, model_id)
        .await?
        .unwrap_or_default();
    let components = model_catalog::list_components(pool, model_id)
        .await?
        .unwrap_or_default();
    let rows = sqlx::query(
        "SELECT source.public_id::text, root.root_key, root.filesystem_path, source.relative_path, \
         content.byte_size, content.sha256, source.lifecycle_state, source.missing_at IS NOT NULL \
         FROM volund.model_source_files link \
         JOIN volund.models model ON model.id=link.model_id \
         JOIN volund.source_files source ON source.id=link.source_file_id \
         JOIN volund.library_roots root ON root.id=source.library_root_id \
         JOIN volund.content_objects content ON content.id=source.content_object_id \
         WHERE model.public_id::text=$1 ORDER BY link.ordinal, source.relative_path",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot load export sources: {error}"))?;
    let mut sources = Vec::with_capacity(rows.len());
    for row in rows {
        let root_key: String = row.get(1);
        let root_path: String = row.get(2);
        let relative_path: String = row.get(3);
        let lifecycle: String = row.get(6);
        let missing: bool = row.get(7);
        let archive_path = archive_path(&root_key, &relative_path)?;
        let (path, exclusion_reason) = if lifecycle != "available" {
            (None, Some("not-available"))
        } else if missing {
            (None, Some("missing"))
        } else {
            match resolve_original(Path::new(&root_path), &relative_path).await {
                Some(path) => (Some(path), None),
                None => (None, Some("missing")),
            }
        };
        sources.push(ExportSource {
            file_id: row.get(0),
            archive_path,
            path,
            exclusion_reason,
            expected_size: row.get(4),
            sha256: row.get(5),
        });
    }
    let slug = model.slug.clone();
    Ok(Some(ModelExportPlan {
        model_id: model.id.clone(),
        slug,
        model: serde_json::to_value(&model).map_err(|error| error.to_string())?,
        components: serde_json::to_value(&components).map_err(|error| error.to_string())?,
        files: files
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?,
        sources,
    }))
}

/// Archive member name for an original; rejects anything that could escape `originals/`.
///
/// # Errors
///
/// Returns a message for absolute, parent-relative or empty paths.
pub fn archive_path(root_key: &str, relative_path: &str) -> Result<String, String> {
    let safe_component = |value: &str| {
        !value.is_empty() && value != "." && value != ".." && !value.contains(['\\', '\0'])
    };
    if !safe_component(root_key) || root_key.contains('/') {
        return Err("export root key is not a safe archive component".to_owned());
    }
    let relative = Path::new(relative_path);
    if relative_path.is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !relative_path.split('/').all(safe_component)
    {
        return Err("export source path is not a safe archive path".to_owned());
    }
    Ok(format!("originals/{root_key}/{relative_path}"))
}

async fn resolve_original(root: &Path, relative_path: &str) -> Option<PathBuf> {
    let root = tokio::fs::canonicalize(root).await.ok()?;
    let canonical = tokio::fs::canonicalize(root.join(relative_path))
        .await
        .ok()?;
    let metadata = tokio::fs::metadata(&canonical).await.ok()?;
    (canonical.starts_with(&root) && metadata.is_file()).then_some(canonical)
}

/// Write the archive to `destination`, verifying every original while copying.
///
/// Blocking: call from a blocking thread.
///
/// # Errors
///
/// Returns a stable error code when the destination exists, an original changed
/// on disk, or the archive cannot be written.
pub fn write_archive(
    destination: &Path,
    plan: &ModelExportPlan,
    created_at_unix_ms: i64,
) -> Result<ArchiveSummary, String> {
    let file = File::options()
        .write(true)
        .read(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| "model_export_stage_failed".to_owned())?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut checksums = String::new();
    let mut results: HashMap<&str, (bool, Option<&'static str>)> = HashMap::new();
    let mut included_files = 0;
    for source in &plan.sources {
        let Some(path) = &source.path else {
            results.insert(&source.file_id, (false, source.exclusion_reason));
            continue;
        };
        zip.start_file(source.archive_path.as_str(), stored)
            .map_err(|_| "model_export_write_failed".to_owned())?;
        copy_verified(path, source, &mut zip)?;
        checksums.push_str(&format!("{}  {}\n", source.sha256, source.archive_path));
        results.insert(&source.file_id, (true, None));
        included_files += 1;
    }
    zip.start_file("SHA256SUMS", deflated)
        .map_err(|_| "model_export_write_failed".to_owned())?;
    zip.write_all(checksums.as_bytes())
        .map_err(|_| "model_export_write_failed".to_owned())?;
    let manifest = manifest(plan, &results, created_at_unix_ms);
    zip.start_file("manifest.json", deflated)
        .map_err(|_| "model_export_write_failed".to_owned())?;
    serde_json::to_writer_pretty(&mut zip, &manifest)
        .map_err(|_| "model_export_write_failed".to_owned())?;
    let writer = zip
        .finish()
        .map_err(|_| "model_export_write_failed".to_owned())?;
    let file = writer
        .into_inner()
        .map_err(|_| "model_export_write_failed".to_owned())?;
    file.sync_all()
        .map_err(|_| "model_export_write_failed".to_owned())?;
    let bytes = file
        .metadata()
        .map_err(|_| "model_export_write_failed".to_owned())?
        .len();
    Ok(ArchiveSummary {
        bytes,
        included_files,
        excluded_files: plan.sources.len() - included_files,
    })
}

fn copy_verified(
    path: &Path,
    source: &ExportSource,
    output: &mut impl Write,
) -> Result<(), String> {
    let changed = || format!("model_export_source_changed:{}", source.file_id);
    let mut input = File::open(path).map_err(|_| changed())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    let expected = u64::try_from(source.expected_size).map_err(|_| changed())?;
    loop {
        let count = input.read(&mut buffer).map_err(|_| changed())?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > expected {
            return Err(changed());
        }
        digest.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| "model_export_write_failed".to_owned())?;
    }
    if copied != expected || format!("{:x}", digest.finalize()) != source.sha256 {
        return Err(changed());
    }
    Ok(())
}

fn manifest(
    plan: &ModelExportPlan,
    results: &HashMap<&str, (bool, Option<&'static str>)>,
    created_at_unix_ms: i64,
) -> Value {
    let by_id: HashMap<&str, &ExportSource> = plan
        .sources
        .iter()
        .map(|source| (source.file_id.as_str(), source))
        .collect();
    let files: Vec<Value> = plan
        .files
        .iter()
        .map(|file| {
            let mut file = file.clone();
            let id = file
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if let (Some(object), Some(source)) = (file.as_object_mut(), by_id.get(id.as_str())) {
                let (included, reason) = results.get(id.as_str()).copied().unwrap_or((false, None));
                object.insert(
                    "archivePath".to_owned(),
                    json!(included.then_some(&source.archive_path)),
                );
                object.insert("sha256".to_owned(), json!(source.sha256));
                object.insert("included".to_owned(), json!(included));
                object.insert("exclusionReason".to_owned(), json!(reason));
            }
            file
        })
        .collect();
    json!({
        "format": FORMAT,
        "formatVersion": FORMAT_VERSION,
        "volundVersion": env!("CARGO_PKG_VERSION"),
        "createdAtUnixMs": created_at_unix_ms,
        "model": plan.model,
        "components": plan.components,
        "files": files,
        "omitted": ["derived-artifacts", "conversion-runs", "previews"],
    })
}

#[cfg(test)]
mod tests {
    use super::archive_path;

    #[test]
    fn archive_paths_stay_below_originals() {
        assert_eq!(
            archive_path("cad", "Projekte/frame/CAD/main.step").as_deref(),
            Ok("originals/cad/Projekte/frame/CAD/main.step")
        );
        for bad in ["../etc/passwd", "/abs", "a//b", "a/./b", "", "a\\b"] {
            assert!(archive_path("cad", bad).is_err(), "{bad} must be rejected");
        }
        assert!(archive_path("../cad", "a.step").is_err());
        assert!(archive_path("ca/d", "a.step").is_err());
    }
}
