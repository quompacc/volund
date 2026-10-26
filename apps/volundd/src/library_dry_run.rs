//! Read-only onboarding dry run for a prospective library root (ADR 0035).
//!
//! It walks the directory the way the scanner would, without hashing, writing,
//! or registering anything, and reports what a first scan would index and what
//! would make it fail.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Extension, State};
use serde::{Deserialize, Serialize};
use volund_core::{CadFormat, normalize_library_path};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::library_admin::{self, LibraryAdminError};
use crate::session::AuthenticatedSession;

const QUARANTINE_DIRECTORY: &str = ".volund-quarantine";
const MAX_ENTRIES: u64 = 500_000;
const MAX_DURATION: Duration = Duration::from_secs(20);
const MAX_LISTED: usize = 20;
const LARGEST: usize = 5;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunRequest {
    pub path: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathIssue {
    /// `directory_unreadable`, `entry_unreadable`, `name_not_utf8`, or `path_invalid`.
    pub code: &'static str,
    pub path: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Overlap {
    pub key: String,
    pub path: String,
    /// `same`, `inside` (the new path lies inside it), or `contains`.
    pub relation: &'static str,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunReport {
    pub canonical_path: String,
    pub files: u64,
    pub bytes: u64,
    pub directories: u64,
    pub cad_files: u64,
    pub formats: BTreeMap<String, u64>,
    pub other_files: u64,
    pub symlinks_skipped: u64,
    /// Problems that make the first scan fail as a whole.
    pub blockers: Vec<PathIssue>,
    pub blocker_count: u64,
    pub largest_files: Vec<LargeFile>,
    pub overlaps: Vec<Overlap>,
    /// The walk stopped at its entry or time bound; counts are lower bounds.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

impl DryRunReport {
    fn block(&mut self, code: &'static str, path: String) {
        self.blocker_count += 1;
        if self.blockers.len() < MAX_LISTED {
            self.blockers.push(PathIssue { code, path });
        }
    }

    fn file(&mut self, relative: String, bytes: u64, format: Option<CadFormat>) {
        self.files += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if let Some(format) = format {
            self.cad_files += 1;
            *self.formats.entry(format.as_str().to_owned()).or_insert(0) += 1;
        } else {
            self.other_files += 1;
        }
        if self.largest_files.len() < LARGEST
            || self
                .largest_files
                .last()
                .is_some_and(|last| bytes > last.bytes)
        {
            self.largest_files.push(LargeFile {
                path: relative,
                bytes,
            });
            self.largest_files.sort_by(|left, right| {
                right
                    .bytes
                    .cmp(&left.bytes)
                    .then(left.path.cmp(&right.path))
            });
            self.largest_files.truncate(LARGEST);
        }
    }
}

fn relative_text(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
        .ok_or_else(|| relative.to_string_lossy().into_owned())
}

/// Walk `root` like the scanner, bounded by entries and time.
#[must_use]
pub fn walk(root: &Path) -> DryRunReport {
    let started = Instant::now();
    let mut report = DryRunReport {
        canonical_path: root.to_string_lossy().into_owned(),
        ..DryRunReport::default()
    };
    let mut pending = vec![root.to_owned()];
    let mut entries_seen = 0_u64;
    'walk: while let Some(directory) = pending.pop() {
        report.directories += 1;
        let Ok(entries) = fs::read_dir(&directory) else {
            let shown = relative_text(root, &directory).unwrap_or_else(|lossy| lossy);
            let shown = if shown.is_empty() {
                ".".to_owned()
            } else {
                shown
            };
            report.block("directory_unreadable", shown);
            continue;
        };
        for entry in entries {
            entries_seen += 1;
            if entries_seen > MAX_ENTRIES || started.elapsed() > MAX_DURATION {
                report.truncated = true;
                break 'walk;
            }
            let Ok(entry) = entry else {
                report.block(
                    "entry_unreadable",
                    relative_text(root, &directory).unwrap_or_else(|lossy| lossy),
                );
                continue;
            };
            if directory == root && entry.file_name() == QUARANTINE_DIRECTORY {
                continue;
            }
            let path: PathBuf = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                report.block(
                    "entry_unreadable",
                    relative_text(root, &path).unwrap_or_else(|lossy| lossy),
                );
                continue;
            };
            if metadata.file_type().is_symlink() {
                report.symlinks_skipped += 1;
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let relative = match relative_text(root, &path) {
                Ok(relative) => relative,
                Err(lossy) => {
                    report.block("name_not_utf8", lossy);
                    continue;
                }
            };
            if normalize_library_path(&relative).is_err() {
                report.block("path_invalid", relative);
                continue;
            }
            let format = path
                .extension()
                .and_then(|value| value.to_str())
                .and_then(CadFormat::from_extension);
            report.file(relative, metadata.len(), format);
        }
    }
    report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    report
}

/// How a prospective root relates to an already registered one.
#[must_use]
pub fn relation(candidate: &Path, existing: &Path) -> Option<&'static str> {
    if candidate == existing {
        Some("same")
    } else if candidate.starts_with(existing) {
        Some("inside")
    } else if existing.starts_with(candidate) {
        Some("contains")
    } else {
        None
    }
}

/// `POST /api/v1/libraries/dry-run`.
pub(crate) async fn route(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Json(request): Json<DryRunRequest>,
) -> Result<Json<DryRunReport>, ApiError> {
    crate::api_auth::require_library_admin(&actor)?;
    let validation = library_admin::validate_path(&request.path).map_err(ApiError::Library)?;
    let root = PathBuf::from(&validation.canonical_path);
    let roots: Vec<(String, String)> = sqlx::query_as(
        "SELECT root_key, filesystem_path FROM volund.library_roots ORDER BY root_key",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| ApiError::Database(format!("cannot list library roots: {error}")))?;
    let walked = root.clone();
    let mut report = tokio::task::spawn_blocking(move || walk(&walked))
        .await
        .map_err(|error| {
            ApiError::Library(LibraryAdminError::BadRequest(format!(
                "dry run failed: {error}"
            )))
        })?;
    for (key, path) in roots {
        let existing = fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(&path));
        if let Some(relation) = relation(&root, &existing) {
            report.overlaps.push(Overlap {
                key,
                path,
                relation,
            });
        }
    }
    Ok(Json(report))
}

#[cfg(test)]
mod tests {
    use super::relation;
    use std::path::Path;

    #[test]
    fn overlapping_roots_are_classified() {
        let library = Path::new("/srv/volund/library");
        assert_eq!(relation(library, library), Some("same"));
        assert_eq!(
            relation(Path::new("/srv/volund/library/cad"), library),
            Some("inside")
        );
        assert_eq!(
            relation(Path::new("/srv/volund"), library),
            Some("contains")
        );
        assert_eq!(relation(Path::new("/srv/volund/library-2"), library), None);
    }
}
