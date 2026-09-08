use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[cfg(unix)]
const COMMAND_TIMEOUT: &str = "2s";
const BLOCKED_FREE_BYTES: u64 = 1024 * 1024 * 1024;
const DEGRADED_USED_PERCENT: u8 = 90;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationalState {
    Healthy,
    Degraded,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReason {
    pub code: &'static str,
    pub state: OperationalState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct StorageObservation {
    pub state: OperationalState,
    pub checked_at_unix_ms: i64,
    pub reachable: bool,
    pub directory: bool,
    pub readable: bool,
    pub writable: bool,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<u8>,
    pub filesystem_type: Option<String>,
    pub reasons: Vec<StorageReason>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
struct ProbeFacts {
    reachable: bool,
    directory: bool,
    readable: bool,
    writable: bool,
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
    used_percent: Option<u8>,
    filesystem_type: Option<String>,
}

/// Observe a configured storage root without creating or changing a file.
#[must_use]
pub fn probe(path: &Path) -> StorageObservation {
    classify(platform_probe(path), now_unix_ms())
}

fn classify(facts: ProbeFacts, checked_at_unix_ms: i64) -> StorageObservation {
    let mut reasons = Vec::new();
    if !facts.reachable {
        reasons.push(reason("storage_unreachable", OperationalState::Blocked));
    } else if !facts.directory {
        reasons.push(reason("storage_not_directory", OperationalState::Blocked));
    }
    if facts.reachable && facts.directory && !facts.readable {
        reasons.push(reason("storage_unreadable", OperationalState::Blocked));
    }
    if facts.reachable && facts.directory && facts.readable && !facts.writable {
        reasons.push(reason("storage_not_writable", OperationalState::Degraded));
    }
    match (facts.available_bytes, facts.used_percent) {
        (Some(available), _) if available < BLOCKED_FREE_BYTES => {
            reasons.push(reason(
                "storage_capacity_blocked",
                OperationalState::Blocked,
            ));
        }
        (_, Some(used)) if used >= DEGRADED_USED_PERCENT => {
            reasons.push(reason("storage_capacity_low", OperationalState::Degraded));
        }
        (None, _) if facts.reachable && facts.directory => {
            reasons.push(reason(
                "storage_capacity_unknown",
                OperationalState::Degraded,
            ));
        }
        _ => {}
    }
    let state = reasons
        .iter()
        .map(|item| item.state)
        .max()
        .unwrap_or(OperationalState::Healthy);
    StorageObservation {
        state,
        checked_at_unix_ms,
        reachable: facts.reachable,
        directory: facts.directory,
        readable: facts.readable,
        writable: facts.writable,
        total_bytes: facts.total_bytes,
        available_bytes: facts.available_bytes,
        used_percent: facts.used_percent,
        filesystem_type: facts.filesystem_type,
        reasons,
    }
}

const fn reason(code: &'static str, state: OperationalState) -> StorageReason {
    StorageReason { code, state }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(unix)]
fn platform_probe(path: &Path) -> ProbeFacts {
    let reachable = command_succeeds("/usr/bin/test", "-e", path);
    let directory = reachable && command_succeeds("/usr/bin/test", "-d", path);
    let readable = reachable
        && directory
        && command_succeeds("/usr/bin/test", "-r", path)
        && command_succeeds("/usr/bin/test", "-x", path);
    let writable = reachable && directory && command_succeeds("/usr/bin/test", "-w", path);
    let capacity = if reachable && directory {
        timed_command(
            "/usr/bin/df",
            &["--block-size=1", "--output=size,avail,pcent,fstype"],
            path,
        )
        .filter(|output| output.status.success())
        .and_then(|output| parse_capacity(&output.stdout))
    } else {
        None
    };
    ProbeFacts {
        reachable,
        directory,
        readable,
        writable,
        total_bytes: capacity.as_ref().map(|value| value.0),
        available_bytes: capacity.as_ref().map(|value| value.1),
        used_percent: capacity.as_ref().map(|value| value.2),
        filesystem_type: capacity.map(|value| value.3),
    }
}

#[cfg(unix)]
fn command_succeeds(program: &str, flag: &str, path: &Path) -> bool {
    Command::new("/usr/bin/timeout")
        .arg(COMMAND_TIMEOUT)
        .arg(program)
        .arg(flag)
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(unix)]
fn timed_command(program: &str, arguments: &[&str], path: &Path) -> Option<Output> {
    Command::new("/usr/bin/timeout")
        .arg(COMMAND_TIMEOUT)
        .arg(program)
        .args(arguments)
        .arg("--")
        .arg(path)
        .output()
        .ok()
}

#[cfg(unix)]
fn parse_capacity(output: &[u8]) -> Option<(u64, u64, u8, String)> {
    let line = String::from_utf8_lossy(output).lines().nth(1)?.to_owned();
    let mut fields = line.split_whitespace();
    let total = fields.next()?.parse().ok()?;
    let available = fields.next()?.parse().ok()?;
    let used = fields.next()?.trim_end_matches('%').parse().ok()?;
    let filesystem = fields.next()?.to_owned();
    Some((total, available, used, filesystem))
}

#[cfg(not(unix))]
fn platform_probe(path: &Path) -> ProbeFacts {
    let metadata = std::fs::metadata(path).ok();
    let directory = metadata.as_ref().is_some_and(std::fs::Metadata::is_dir);
    let readable = directory && std::fs::read_dir(path).is_ok();
    let writable = metadata
        .as_ref()
        .is_some_and(|value| !value.permissions().readonly());
    ProbeFacts {
        reachable: metadata.is_some(),
        directory,
        readable,
        writable,
        ..ProbeFacts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationalState, ProbeFacts, classify};

    fn healthy_facts() -> ProbeFacts {
        ProbeFacts {
            reachable: true,
            directory: true,
            readable: true,
            writable: true,
            total_bytes: Some(100 * 1024 * 1024 * 1024),
            available_bytes: Some(50 * 1024 * 1024 * 1024),
            used_percent: Some(50),
            filesystem_type: Some("ext4".to_owned()),
        }
    }

    #[test]
    fn state_aggregation_uses_the_most_severe_reason() {
        assert_eq!(
            classify(healthy_facts(), 7).state,
            OperationalState::Healthy
        );
        let mut degraded = healthy_facts();
        degraded.writable = false;
        assert_eq!(classify(degraded, 7).state, OperationalState::Degraded);
        let mut blocked = healthy_facts();
        blocked.readable = false;
        blocked.writable = false;
        assert_eq!(classify(blocked, 7).state, OperationalState::Blocked);
    }

    #[test]
    fn low_and_unknown_capacity_are_truthful() {
        let mut low = healthy_facts();
        low.available_bytes = Some(512 * 1024 * 1024);
        assert_eq!(classify(low, 7).state, OperationalState::Blocked);
        let mut unknown = healthy_facts();
        unknown.available_bytes = None;
        unknown.used_percent = None;
        assert_eq!(classify(unknown, 7).state, OperationalState::Degraded);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_probe_distinguishes_directories_files_and_missing_paths() {
        let root =
            std::env::temp_dir().join(format!("volund-storage-probe-{}", std::process::id()));
        std::fs::create_dir(&root).expect("create probe directory");
        let directory = super::probe(&root);
        assert!(directory.reachable);
        assert!(directory.directory);
        assert!(directory.readable);
        assert!(directory.total_bytes.is_some());

        let file = root.join("ordinary-file");
        std::fs::write(&file, b"probe").expect("write isolated fixture");
        let ordinary_file = super::probe(&file);
        assert_eq!(ordinary_file.state, OperationalState::Blocked);
        assert!(ordinary_file.reachable);
        assert!(!ordinary_file.directory);

        std::fs::remove_file(file).expect("remove fixture file");
        std::fs::remove_dir(&root).expect("remove probe directory");
        let missing = super::probe(&root);
        assert_eq!(missing.state, OperationalState::Blocked);
        assert!(!missing.reachable);
    }
}
