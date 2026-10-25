//! Removal of conversion directories that no catalog row references.
//!
//! The runner leaves `.work/<job>` behind when its worker is killed, cancelled,
//! or stopped. It keeps `failed/<job>` for diagnosis, and a cancelled or lost
//! run can leave an unpublished `jobs/<job>`. Retention removes them once they
//! are old enough that no live conversion can still own them (ADR 0030).

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use sqlx::PgPool;

/// A running conversion lasts at most its 24-hour limit plus grace periods.
const WORK_MIN_AGE: Duration = Duration::from_secs(2 * 24 * 60 * 60);
/// Publication commits seconds after the runner's rename into `jobs/`.
const UNREFERENCED_JOB_MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ReapReport {
    pub(crate) directories: i64,
    pub(crate) bytes: i64,
    pub(crate) codes: Vec<String>,
}

/// Remove stale `.work`, aged `failed`, and unreferenced `jobs` directories.
/// `root` must already be canonical; `failed_days` is the artifact retention age.
pub(crate) async fn reap(
    pool: &PgPool,
    root: &Path,
    failed_days: i64,
) -> Result<ReapReport, String> {
    let referenced: HashSet<String> = sqlx::query_scalar(
        "SELECT DISTINCT split_part(relative_path, '/', 2) FROM volund.derived_artifacts \
         WHERE relative_path LIKE 'jobs/%'",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list referenced conversion directories: {error}"))?
    .into_iter()
    .collect();
    let failed_age = Duration::from_secs(u64::try_from(failed_days.max(1)).unwrap_or(1) * 86_400);
    let mut report = ReapReport::default();
    for (area, min_age) in [
        (".work", WORK_MIN_AGE),
        ("failed", failed_age),
        ("jobs", UNREFERENCED_JOB_MIN_AGE),
    ] {
        let skip = if area == "jobs" {
            Some(&referenced)
        } else {
            None
        };
        reap_area(&root.join(area), root, min_age, skip, &mut report);
    }
    Ok(report)
}

fn reap_area(
    directory: &Path,
    root: &Path,
    min_age: Duration,
    referenced: Option<&HashSet<String>>,
    report: &mut ReapReport,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_job_id(&name) || referenced.is_some_and(|ids| ids.contains(&name)) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir() {
            continue;
        }
        let old_enough = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= min_age);
        if !old_enough {
            continue;
        }
        match fs::canonicalize(&path) {
            Ok(canonical)
                if canonical.starts_with(root) && canonical.parent() == Some(directory) =>
            {
                let bytes = tree_bytes(&canonical);
                if fs::remove_dir_all(&canonical).is_ok() {
                    report.directories += 1;
                    report.bytes += bytes;
                } else {
                    report
                        .codes
                        .push("orphan_directory_remove_failed".to_owned());
                }
            }
            _ => report
                .codes
                .push("orphan_directory_path_refused".to_owned()),
        }
    }
}

/// Runner job ids are `<unix-ms>-<pid>`; anything else is left untouched.
fn is_job_id(name: &str) -> bool {
    name.split_once('-').is_some_and(|(time, pid)| {
        !time.is_empty()
            && !pid.is_empty()
            && time.bytes().all(|byte| byte.is_ascii_digit())
            && pid.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Total size of regular files below `path`, without following symlinks.
fn tree_bytes(path: &Path) -> i64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            if metadata.is_dir() {
                Some(tree_bytes(&entry.path()))
            } else if metadata.is_file() {
                i64::try_from(metadata.len()).ok()
            } else {
                Some(0)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::is_job_id;

    #[test]
    fn only_runner_job_ids_are_eligible() {
        assert!(is_job_id("1761400000000-4242"));
        for name in [
            "", "-", "1-", "-1", "job", "1-2-3", "../1-2", "1-2 ", "lock",
        ] {
            assert!(!is_job_id(name), "{name:?}");
        }
    }
}
