use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CONVERTER: &str = "/usr/local/bin/volund-cad-convert";
const DEFAULT_DERIVED_ROOT: &str = "/srv/volund/derived";
const DEFAULT_SCRATCH_ROOT: &str = "/srv/volund/scratch";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
/// Grace between the timeout's SIGTERM and its SIGKILL of the converter.
const KILL_AFTER_SECONDS: u64 = 30;
const FLOCK: &str = "/usr/bin/flock";
const TIMEOUT: &str = "/usr/bin/timeout";
/// Exit status of `convert` and `_execute-job` when the conversion timed out,
/// matching GNU timeout.
pub const TIMED_OUT_EXIT: u8 = 124;
/// Placeholder for an absent optional argument of the internal executor.
const ABSENT: &str = "-";

/// A failed conversion job and whether it failed by exceeding its time limit.
#[derive(Debug)]
pub struct JobFailure {
    pub message: String,
    pub timed_out: bool,
}

impl From<String> for JobFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            timed_out: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ConvertOptions {
    input: PathBuf,
    profile: String,
    timeout_seconds: u64,
    derived_root: PathBuf,
    scratch_root: PathBuf,
    converter: PathBuf,
    linear_deflection: Option<f64>,
    angular_deflection: Option<f64>,
}

#[derive(Clone, Debug)]
struct ExecutionOptions {
    job_id: String,
    submitted_at_ms: u128,
    input: PathBuf,
    work_dir: PathBuf,
    ready_dir: PathBuf,
    failed_dir: PathBuf,
    profile: String,
    timeout_seconds: u64,
    converter: PathBuf,
    linear_deflection: Option<f64>,
    angular_deflection: Option<f64>,
}

pub fn print_usage() {
    eprintln!(
        "usage:\n  \
         volundd doctor\n  \
         volundd contract-version\n  \
         volundd migrate\n  \
         volundd database-doctor\n  \
         volundd serve\n  \
         volundd register-root --key <key> --name <name> --path <absolute-path>\n  \
         volundd scan --root <key> [--full]\n  \
         volundd enqueue-preview --file <public-id> [--profile web|fine]\n  \
         volundd process-next-preview\n  \
         volundd process-next-scan\n  \
         volundd run-import-cleanup\n  \
         volundd convert --input model.cad [--profile web|fine] \
         [--timeout-seconds 1800] [--derived-root /srv/volund/derived] \
         [--scratch-root /srv/volund/scratch] \
         [--converter /usr/local/bin/volund-cad-convert] \
         [--linear-deflection <mm>] [--angular-deflection <rad>]"
    );
}

pub fn run_convert(arguments: impl Iterator<Item = OsString>) -> Result<(), JobFailure> {
    let options = parse_convert_options(arguments)?;
    validate_runtime(&options)?;

    let input = fs::canonicalize(&options.input)
        .map_err(|error| format!("cannot resolve input {}: {error}", options.input.display()))?;
    if !input.is_file() {
        return Err(format!("input is not a regular file: {}", input.display()).into());
    }

    fs::create_dir_all(&options.derived_root).map_err(|error| {
        format!(
            "cannot create derived root {}: {error}",
            options.derived_root.display()
        )
    })?;
    fs::create_dir_all(&options.scratch_root).map_err(|error| {
        format!(
            "cannot create scratch root {}: {error}",
            options.scratch_root.display()
        )
    })?;

    let work_root = options.derived_root.join(".work");
    let ready_root = options.derived_root.join("jobs");
    let failed_root = options.derived_root.join("failed");
    for directory in [&work_root, &ready_root, &failed_root] {
        fs::create_dir_all(directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        set_mode(directory, 0o750)?;
    }

    let submitted_at_ms = unix_time_ms()?;
    let job_id = format!("{submitted_at_ms}-{}", std::process::id());
    let work_dir = work_root.join(&job_id);
    let ready_dir = ready_root.join(&job_id);
    let failed_dir = failed_root.join(&job_id);
    fs::create_dir(&work_dir).map_err(|error| {
        format!(
            "cannot create job directory {}: {error}",
            work_dir.display()
        )
    })?;
    set_mode(&work_dir, 0o750)?;
    write_job_metadata(
        &work_dir,
        &job_id,
        "queued",
        &input,
        &options.profile,
        submitted_at_ms,
        "waiting for the conversion slot",
    )?;

    println!("job_id={job_id}");
    println!("state=queued");

    let current_executable = env::current_exe()
        .map_err(|error| format!("cannot resolve volundd executable: {error}"))?;
    let lock_file = options.scratch_root.join("cad-convert.lock");
    let status = Command::new(FLOCK)
        .arg("--exclusive")
        .arg(&lock_file)
        .arg(current_executable)
        .arg("_execute-job")
        .arg(&job_id)
        .arg(submitted_at_ms.to_string())
        .arg(&input)
        .arg(&work_dir)
        .arg(&ready_dir)
        .arg(&failed_dir)
        .arg(&options.profile)
        .arg(options.timeout_seconds.to_string())
        .arg(&options.converter)
        .arg(optional_argument(options.linear_deflection))
        .arg(optional_argument(options.angular_deflection))
        .status()
        .map_err(|error| format!("cannot start flock-controlled job: {error}"))?;

    if status.success() {
        println!("state=ready");
        println!("output={}", ready_dir.display());
        Ok(())
    } else if status.code() == Some(i32::from(TIMED_OUT_EXIT)) {
        println!("state=timed-out");
        println!("output={}", failed_dir.display());
        Err(JobFailure {
            message: format!(
                "conversion exceeded the {} second limit",
                options.timeout_seconds
            ),
            timed_out: true,
        })
    } else {
        println!("state=failed");
        println!("output={}", failed_dir.display());
        Err(exit_status_message(status).into())
    }
}

fn optional_argument(value: Option<f64>) -> String {
    value.map_or_else(|| ABSENT.to_owned(), |value| value.to_string())
}

pub fn execute_job(arguments: impl Iterator<Item = OsString>) -> Result<(), JobFailure> {
    let options = parse_execution_options(arguments)?;
    // The supervising worker starts the conversion deadline at this line, so
    // time spent waiting for the global conversion lock is not counted.
    println!("state=running");
    write_job_metadata(
        &options.work_dir,
        &options.job_id,
        "running",
        &options.input,
        &options.profile,
        options.submitted_at_ms,
        "native OCCT conversion is running",
    )?;

    let mut timeout = Command::new(TIMEOUT);
    // The preview supervisor owns the whole group, including converter children.
    // Standalone CLI conversions retain timeout's own process-group handling.
    if env::var_os("VOLUND_MANAGED_PROCESS_GROUP").as_deref() == Some(std::ffi::OsStr::new("1")) {
        timeout.arg("--foreground");
    }
    timeout
        .arg("--signal=TERM")
        .arg(format!("--kill-after={KILL_AFTER_SECONDS}s"))
        .arg(format!("{}s", options.timeout_seconds))
        .arg(&options.converter)
        .arg("convert")
        .arg("--input")
        .arg(&options.input)
        .arg("--output")
        .arg(&options.work_dir)
        .arg("--profile")
        .arg(&options.profile);
    if let Some(value) = options.linear_deflection {
        timeout.arg("--linear-deflection").arg(value.to_string());
    }
    if let Some(value) = options.angular_deflection {
        timeout.arg("--angular-deflection").arg(value.to_string());
    }
    let status = timeout
        .status()
        .map_err(|error| format!("cannot start CAD converter: {error}"))?;
    secure_tree(&options.work_dir)?;

    if status.success() && options.work_dir.join("result.json").is_file() {
        write_job_metadata(
            &options.work_dir,
            &options.job_id,
            "ready",
            &options.input,
            &options.profile,
            options.submitted_at_ms,
            "conversion completed",
        )?;
        fs::rename(&options.work_dir, &options.ready_dir)
            .map_err(|error| format!("cannot publish job {} as ready: {error}", options.job_id))?;
        return Ok(());
    }

    let timed_out = status.code() == Some(i32::from(TIMED_OUT_EXIT));
    let message = if timed_out {
        format!(
            "conversion exceeded the {} second limit",
            options.timeout_seconds
        )
    } else if status.success() {
        "converter returned success without result.json".to_owned()
    } else {
        exit_status_message(status)
    };
    write_job_metadata(
        &options.work_dir,
        &options.job_id,
        "failed",
        &options.input,
        &options.profile,
        options.submitted_at_ms,
        &message,
    )?;
    fs::rename(&options.work_dir, &options.failed_dir)
        .map_err(|error| format!("cannot publish job {} as failed: {error}", options.job_id))?;
    Err(JobFailure { message, timed_out })
}

fn parse_convert_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<ConvertOptions, String> {
    let mut options = ConvertOptions {
        input: PathBuf::new(),
        profile: "web".to_owned(),
        timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        derived_root: PathBuf::from(DEFAULT_DERIVED_ROOT),
        scratch_root: PathBuf::from(DEFAULT_SCRATCH_ROOT),
        converter: PathBuf::from(DEFAULT_CONVERTER),
        linear_deflection: None,
        angular_deflection: None,
    };
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{} requires a value", argument.to_string_lossy()))?;
        match argument.to_string_lossy().as_ref() {
            "--input" => options.input = PathBuf::from(value),
            "--profile" => options.profile = value.to_string_lossy().into_owned(),
            "--timeout-seconds" => {
                options.timeout_seconds = value
                    .to_string_lossy()
                    .parse()
                    .map_err(|_| "--timeout-seconds must be an integer".to_owned())?;
            }
            "--derived-root" => options.derived_root = PathBuf::from(value),
            "--scratch-root" => options.scratch_root = PathBuf::from(value),
            "--converter" => options.converter = PathBuf::from(value),
            "--linear-deflection" => {
                options.linear_deflection = Some(parse_deflection(&argument, &value)?);
            }
            "--angular-deflection" => {
                options.angular_deflection = Some(parse_deflection(&argument, &value)?);
            }
            unknown => return Err(format!("unknown option: {unknown}")),
        }
    }
    if options.input.as_os_str().is_empty() {
        return Err("--input is required".to_owned());
    }
    if options.profile != "web" && options.profile != "fine" {
        return Err("--profile must be web or fine".to_owned());
    }
    if !(1..=86_400).contains(&options.timeout_seconds) {
        return Err("--timeout-seconds must be between 1 and 86400".to_owned());
    }
    for (name, path) in [
        ("--derived-root", &options.derived_root),
        ("--scratch-root", &options.scratch_root),
        ("--converter", &options.converter),
    ] {
        if !is_linux_absolute(path) {
            return Err(format!("{name} must be an absolute path"));
        }
    }
    Ok(options)
}

fn parse_deflection(name: &OsString, value: &OsString) -> Result<f64, String> {
    value
        .to_string_lossy()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("{} must be a positive number", name.to_string_lossy()))
}

fn is_linux_absolute(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().starts_with('/')
}

fn parse_execution_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<ExecutionOptions, String> {
    let arguments: Vec<_> = arguments.collect();
    if arguments.len() != 11 {
        return Err("invalid internal executor arguments".to_owned());
    }
    let text = |index: usize| arguments[index].to_string_lossy().into_owned();
    let optional = |index: usize| -> Result<Option<f64>, String> {
        if arguments[index] == ABSENT {
            return Ok(None);
        }
        parse_deflection(&OsString::from("internal deflection"), &arguments[index]).map(Some)
    };
    Ok(ExecutionOptions {
        job_id: text(0),
        submitted_at_ms: text(1)
            .parse()
            .map_err(|_| "invalid internal submission time".to_owned())?,
        input: PathBuf::from(&arguments[2]),
        work_dir: PathBuf::from(&arguments[3]),
        ready_dir: PathBuf::from(&arguments[4]),
        failed_dir: PathBuf::from(&arguments[5]),
        profile: text(6),
        timeout_seconds: text(7)
            .parse()
            .map_err(|_| "invalid internal timeout".to_owned())?,
        converter: PathBuf::from(&arguments[8]),
        linear_deflection: optional(9)?,
        angular_deflection: optional(10)?,
    })
}

fn validate_runtime(options: &ConvertOptions) -> Result<(), String> {
    for (name, path) in [
        ("flock", Path::new(FLOCK)),
        ("timeout", Path::new(TIMEOUT)),
        ("converter", options.converter.as_path()),
    ] {
        if !path.is_file() {
            return Err(format!(
                "required {name} executable is missing: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn write_job_metadata(
    directory: &Path,
    job_id: &str,
    state: &str,
    input: &Path,
    profile: &str,
    submitted_at_ms: u128,
    message: &str,
) -> Result<(), String> {
    let updated_at_ms = unix_time_ms()?;
    let document = format!(
        "{{\n  \"jobId\": \"{}\",\n  \"state\": \"{}\",\n  \
         \"input\": \"{}\",\n  \"profile\": \"{}\",\n  \
         \"submittedAtUnixMs\": {},\n  \"updatedAtUnixMs\": {},\n  \
         \"message\": \"{}\"\n}}\n",
        json_escape(job_id),
        json_escape(state),
        json_escape(&input.to_string_lossy()),
        json_escape(profile),
        submitted_at_ms,
        updated_at_ms,
        json_escape(message)
    );
    let path = directory.join("job.json");
    let temporary_path = directory.join("job.json.tmp");
    fs::write(&temporary_path, document).map_err(|error| {
        format!(
            "cannot write job metadata in {}: {error}",
            directory.display()
        )
    })?;
    set_mode(&temporary_path, 0o640)?;
    fs::rename(&temporary_path, &path).map_err(|error| {
        format!(
            "cannot publish job metadata in {}: {error}",
            directory.display()
        )
    })?;
    set_mode(&path, 0o640)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

fn secure_tree(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "job output contains a symbolic link: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        set_mode(path, 0o750)?;
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?
        {
            secure_tree(
                &entry
                    .map_err(|error| format!("cannot read entry in {}: {error}", path.display()))?
                    .path(),
            )?;
        }
    } else if metadata.is_file() {
        set_mode(path, 0o640)?;
    } else {
        return Err(format!("unsupported job output type: {}", path.display()));
    }
    Ok(())
}

fn unix_time_ms() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write;
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn exit_status_message(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "conversion process terminated by a signal".to_owned(),
        |code| format!("conversion process exited with status {code}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings<'a>(values: &'a [&'a str]) -> impl Iterator<Item = OsString> + 'a {
        values.iter().map(OsString::from)
    }

    #[test]
    fn convert_defaults_are_safe_for_the_native_lxc() {
        let options =
            parse_convert_options(strings(&["--input", "/tmp/model.step"])).expect("valid options");
        assert_eq!(options.profile, "web");
        assert_eq!(options.timeout_seconds, 1800);
        assert_eq!(options.derived_root, PathBuf::from(DEFAULT_DERIVED_ROOT));
        assert_eq!(options.scratch_root, PathBuf::from(DEFAULT_SCRATCH_ROOT));
    }

    #[test]
    fn invalid_profile_and_relative_roots_are_refused() {
        assert!(
            parse_convert_options(strings(&[
                "--input",
                "/tmp/model.step",
                "--profile",
                "enormous"
            ]))
            .is_err()
        );
        assert!(
            parse_convert_options(strings(&[
                "--input",
                "/tmp/model.step",
                "--derived-root",
                "relative"
            ]))
            .is_err()
        );
    }

    #[test]
    fn custom_profile_deflections_are_accepted_and_validated() {
        let options = parse_convert_options(strings(&[
            "--input",
            "/tmp/model.step",
            "--linear-deflection",
            "0.05",
            "--angular-deflection",
            "0.3",
        ]))
        .expect("deflections are valid options");
        assert_eq!(options.linear_deflection, Some(0.05));
        assert_eq!(options.angular_deflection, Some(0.3));
        assert_eq!(optional_argument(options.linear_deflection), "0.05");
        assert_eq!(optional_argument(None), ABSENT);
        for value in ["0", "-1", "NaN", "fine"] {
            assert!(
                parse_convert_options(strings(&[
                    "--input",
                    "/tmp/model.step",
                    "--linear-deflection",
                    value
                ]))
                .is_err(),
                "{value} must be refused"
            );
        }
    }

    #[test]
    fn job_metadata_strings_are_validly_escaped() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }
}
