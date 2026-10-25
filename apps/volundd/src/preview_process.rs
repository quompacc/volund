use std::process::{ExitStatus, Stdio};

use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
    time::{Duration, Instant, sleep, sleep_until},
};

/// Prefix of every timeout diagnostic; the worker maps it to `timed-out`.
pub(crate) const TIMED_OUT: &str = "conversion exceeded the";
/// Mirrors the runner's TERM-to-KILL grace (`job_runner::KILL_AFTER_SECONDS`).
const RUNNER_KILL_AFTER_SECONDS: u64 = 30;
/// Extra time for the runner to record and publish its own timeout.
const RUNNER_MARGIN_SECONDS: u64 = 5;
/// How long a descendant may keep the output pipes open after the runner exits.
const PIPE_DRAIN: Duration = Duration::from_secs(2);

pub(crate) struct RunnerOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

// Keep this guard alive until output collection ends. Dropping a cancelled
// future must stop descendants as well as the direct runner process.
#[cfg_attr(
    not(unix),
    expect(dead_code, reason = "process-group termination is Unix-only")
)]
struct ProcessGroup(u32);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{}", self.0)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn timed_out(timeout_seconds: u64) -> String {
    format!("{TIMED_OUT} {timeout_seconds} second limit")
}

/// Run the guarded conversion runner and supervise it until it exits.
///
/// The conversion deadline starts when the runner reports `state=running`,
/// that is after it obtained the global conversion lock. The runner's own
/// timeout fires first and publishes a failed job; this outer bound only
/// catches a runner that cannot finish its own timeout handling.
pub(crate) async fn run(
    mut command: Command,
    pool: &PgPool,
    run_id: i64,
    timeout_seconds: u64,
) -> Result<RunnerOutput, String> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .env("VOLUND_MANAGED_PROCESS_GROUP", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start guarded conversion runner: {error}"))?;
    let _group = ProcessGroup(child.id().ok_or("conversion runner has no process ID")?);
    let mut lines = BufReader::new(child.stdout.take().ok_or("runner stdout is missing")?).lines();
    let mut stderr_pipe = child.stderr.take().ok_or("runner stderr is missing")?;
    let stderr = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr_pipe.read_to_string(&mut text).await;
        text
    });
    let bound =
        Duration::from_secs(timeout_seconds + RUNNER_KILL_AFTER_SECONDS + RUNNER_MARGIN_SECONDS);
    let mut deadline: Option<Instant> = None;
    let mut stdout = String::new();
    let mut stdout_open = true;
    let mut status = None;
    let mut drained_by = None;
    loop {
        if !stdout_open && status.is_some() {
            break;
        }
        let limit = deadline.or(drained_by);
        tokio::select! {
            line = lines.next_line(), if stdout_open => match line {
                Ok(Some(line)) => {
                    if line == "state=running" && deadline.is_none() {
                        deadline = Some(Instant::now() + bound);
                    }
                    stdout.push_str(&line);
                    stdout.push('\n');
                }
                Ok(None) | Err(_) => stdout_open = false,
            },
            result = child.wait(), if status.is_none() => {
                status = Some(result.map_err(|error| format!("cannot collect conversion output: {error}"))?);
                drained_by = Some(Instant::now() + PIPE_DRAIN);
                deadline = None;
            }
            () = sleep_until(limit.unwrap_or_else(Instant::now)), if limit.is_some() => {
                if status.is_some() {
                    break;
                }
                return Err(timed_out(timeout_seconds));
            }
            () = sleep(Duration::from_millis(100)) => {
                let cancelled: bool = sqlx::query_scalar(
                    "SELECT cancellation_requested_at IS NOT NULL FROM volund.conversion_runs WHERE id=$1",
                ).bind(run_id).fetch_one(pool).await
                    .map_err(|error| format!("cannot inspect conversion cancellation: {error}"))?;
                if cancelled {
                    return Err("conversion cancellation requested".to_owned());
                }
            }
        }
    }
    let status = status.ok_or("conversion runner did not exit")?;
    let stderr = match tokio::time::timeout(PIPE_DRAIN, stderr).await {
        Ok(Ok(text)) => text,
        _ => String::new(),
    };
    if stdout.lines().any(|line| line == "state=timed-out") {
        return Err(timed_out(timeout_seconds));
    }
    Ok(RunnerOutput {
        status,
        stdout,
        stderr,
    })
}
