//! `process-next-preview` and `process-next-scan`: one bounded worker batch per
//! systemd timer tick, with an orderly stop on SIGTERM or SIGINT (ADR 0030).

use std::process::ExitCode;

use tokio_util::sync::CancellationToken;
use volundd::{database, operations, preview_pipeline, runtime_log, scan_pipeline};

#[derive(Clone, Copy)]
pub(crate) enum Worker {
    Preview,
    Scan,
}

impl Worker {
    const fn component(self) -> &'static str {
        match self {
            Self::Preview => "preview-worker",
            Self::Scan => "scan-worker",
        }
    }

    const fn event_prefix(self) -> &'static str {
        match self {
            Self::Preview => "preview_worker",
            Self::Scan => "scan_worker",
        }
    }

    const fn start_failed(self) -> &'static str {
        match self {
            Self::Preview => "preview_worker.start_failed",
            Self::Scan => "scan_worker.start_failed",
        }
    }

    const fn heartbeat_failed(self) -> &'static str {
        match self {
            Self::Preview => "preview_worker.heartbeat_failed",
            Self::Scan => "scan_worker.heartbeat_failed",
        }
    }

    const fn job_kind(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Scan => "scan",
        }
    }
}

/// Cancel the returned token when the service manager asks the worker to stop.
fn stop_on_signal() -> CancellationToken {
    let stop = CancellationToken::new();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        for kind in [SignalKind::terminate(), SignalKind::interrupt()] {
            if let Ok(mut stream) = signal(kind) {
                let stop = stop.clone();
                tokio::spawn(async move {
                    stream.recv().await;
                    stop.cancel();
                });
            }
        }
    }
    stop
}

pub(crate) async fn run(worker: Worker) -> ExitCode {
    let component = worker.component();
    let stop = stop_on_signal();
    let preview_config = match worker {
        Worker::Preview => match preview_pipeline::PreviewWorkerConfig::from_environment() {
            Ok(config) => Some(config),
            Err(message) => {
                runtime_log::failure(
                    component,
                    worker.start_failed(),
                    "worker_config_invalid",
                    &message,
                );
                return ExitCode::FAILURE;
            }
        },
        Worker::Scan => None,
    };
    let pool = match database::DatabaseConfig::from_environment() {
        Ok(config) => match database::connect(&config).await {
            Ok(pool) => pool,
            Err(message) => {
                runtime_log::failure(
                    component,
                    worker.start_failed(),
                    "database_connect_failed",
                    &message,
                );
                return ExitCode::FAILURE;
            }
        },
        Err(message) => {
            runtime_log::failure(
                component,
                worker.start_failed(),
                "database_config_invalid",
                &message,
            );
            return ExitCode::FAILURE;
        }
    };
    let result: Result<Vec<(String, String)>, String> = match &preview_config {
        Some(config) => preview_pipeline::process_batch(&pool, config, &stop)
            .await
            .map(|reports| reports.into_iter().map(|r| (r.id, r.status)).collect()),
        None => scan_pipeline::process_batch(&pool, &stop)
            .await
            .map(|reports| reports.into_iter().map(|r| (r.id, r.status)).collect()),
    };
    if let Err(message) = operations::record_component(&pool, component, result.is_ok()).await {
        runtime_log::failure(
            component,
            worker.heartbeat_failed(),
            "heartbeat_failed",
            &message,
        );
        return ExitCode::FAILURE;
    }
    report(&pool, worker, result, stop.is_cancelled()).await
}

/// Log the batch outcome and map it to the process exit code.
async fn report(
    pool: &sqlx::PgPool,
    worker: Worker,
    result: Result<Vec<(String, String)>, String>,
    stopped: bool,
) -> ExitCode {
    let component = worker.component();
    let prefix = worker.event_prefix();
    if stopped {
        runtime_log::record(
            pool,
            component,
            &format!("{prefix}.stopped"),
            "worker_stopped",
            runtime_log::Severity::Info,
            "stop requested; interrupted jobs were handed back to the queue",
            None,
        )
        .await;
    }
    match result {
        Ok(reports) if !reports.is_empty() => {
            for (id, status) in &reports {
                runtime_log::record(
                    pool,
                    component,
                    &format!("{prefix}.job.completed"),
                    &format!("{}_job_terminal", worker.job_kind()),
                    runtime_log::Severity::Info,
                    &format!("state={status}"),
                    Some(id),
                )
                .await;
            }
            ExitCode::SUCCESS
        }
        Ok(_) => {
            runtime_log::record(
                pool,
                component,
                &format!("{prefix}.idle"),
                "queue_empty",
                runtime_log::Severity::Debug,
                &format!("no queued {} job", worker.job_kind()),
                None,
            )
            .await;
            ExitCode::SUCCESS
        }
        Err(message) => {
            runtime_log::record(
                pool,
                component,
                &format!("{prefix}.failed"),
                &format!("{prefix}_failed"),
                runtime_log::Severity::Error,
                &message,
                None,
            )
            .await;
            ExitCode::FAILURE
        }
    }
}
