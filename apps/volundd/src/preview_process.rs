use std::process::{Output, Stdio};

use sqlx::PgPool;
use tokio::{
    process::Command,
    time::{Duration, sleep},
};

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

pub(crate) async fn run(
    mut command: Command,
    pool: &PgPool,
    run_id: i64,
    timeout_seconds: u64,
) -> Result<Output, String> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .env("VOLUND_MANAGED_PROCESS_GROUP", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| format!("cannot start guarded conversion runner: {error}"))?;
    let _group = ProcessGroup(child.id().ok_or("conversion runner has no process ID")?);
    let output = child.wait_with_output();
    tokio::pin!(output);
    // Also bound stuck flock waiters and descendants holding stdout open after
    // timeout has terminated the converter's immediate parent.
    let deadline = sleep(Duration::from_secs(timeout_seconds.saturating_add(1)));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            result = &mut output => {
                return result.map_err(|error| format!("cannot collect conversion output: {error}"));
            }
            () = &mut deadline => {
                return Err(format!("conversion exceeded the {timeout_seconds} second limit"));
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
}
