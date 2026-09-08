mod job_runner;

use std::env;
use std::process::ExitCode;

use volund_core::CAD_CONVERT_CONTRACT_VERSION;
use volundd::{
    catalog_options, daemon_command, database, import_cleanup, operations, policy_worker,
    preview_options, preview_pipeline, runtime_log, scan_pipeline, scanner,
};

#[tokio::main]
#[allow(clippy::too_many_lines)] // One flat command dispatcher; domain logic lives in modules.
async fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    match arguments.next().as_deref() {
        Some(command) if command == "doctor" => {
            println!("VÖLUND daemon: ok");
            println!("CAD converter contract: v{CAD_CONVERT_CONTRACT_VERSION}");
            println!("Deployment target: native Linux/systemd");
            ExitCode::SUCCESS
        }
        Some(command) if command == "contract-version" => {
            println!("{CAD_CONVERT_CONTRACT_VERSION}");
            ExitCode::SUCCESS
        }
        Some(command) if command == "migrate" => {
            let config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    eprintln!("database configuration failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            match database::connect(&config).await {
                Ok(pool) => match database::run_migrations(&pool).await {
                    Ok(applied) => {
                        println!("database migration: ok");
                        println!("applied migrations: {applied}");
                        ExitCode::SUCCESS
                    }
                    Err(message) => {
                        eprintln!("{message}");
                        ExitCode::FAILURE
                    }
                },
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "database-doctor" => {
            let config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    eprintln!("database configuration failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            match database::connect(&config).await {
                Ok(pool) => match database::inspect(&pool).await {
                    Ok(health) => {
                        println!("PostgreSQL database: {}", health.database);
                        println!("PostgreSQL role: {}", health.role);
                        println!("PostgreSQL server: {}", health.server_version_num);
                        println!("VÖLUND schema tables: {}", health.schema_table_count);
                        println!("Applied migrations: {}", health.applied_migrations);
                        if health.is_ready() {
                            println!("Database health: ok");
                            ExitCode::SUCCESS
                        } else {
                            eprintln!("Database health: migration required");
                            ExitCode::FAILURE
                        }
                    }
                    Err(message) => {
                        eprintln!("{message}");
                        ExitCode::FAILURE
                    }
                },
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "serve" => daemon_command::run().await,
        Some(command) if command == "register-root" => {
            let options = match catalog_options::parse_register_root_options(arguments) {
                Ok(options) => options,
                Err(message) => {
                    eprintln!("invalid root registration: {message}");
                    return ExitCode::from(2);
                }
            };
            let config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    eprintln!("database configuration failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(pool) => pool,
                Err(message) => {
                    eprintln!("{message}");
                    return ExitCode::FAILURE;
                }
            };
            match scanner::register_root(&pool, &options.key, &options.name, &options.path).await {
                Ok(path) => {
                    println!("library root: {}", options.key);
                    println!("filesystem path: {}", path.display());
                    println!("registration: ok");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("root registration failed: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "scan" => {
            let options = match catalog_options::parse_scan_options(arguments) {
                Ok(options) => options,
                Err(message) => {
                    eprintln!("invalid scan request: {message}");
                    return ExitCode::from(2);
                }
            };
            let config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    eprintln!("database configuration failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(pool) => pool,
                Err(message) => {
                    eprintln!("{message}");
                    return ExitCode::FAILURE;
                }
            };
            match scanner::scan_root(&pool, &options.root_key, options.full).await {
                Ok(report) => {
                    println!("library root: {}", options.root_key);
                    println!("discovered files: {}", report.discovered_files);
                    println!("hashed files: {}", report.hashed_files);
                    println!("missing files: {}", report.missing_files);
                    println!("scan: ok");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("scan failed: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "enqueue-preview" => {
            let options = match preview_options::parse_enqueue_options(arguments) {
                Ok(options) => options,
                Err(message) => {
                    eprintln!("invalid preview request: {message}");
                    return ExitCode::from(2);
                }
            };
            let config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    eprintln!("database configuration failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(pool) => pool,
                Err(message) => {
                    eprintln!("{message}");
                    return ExitCode::FAILURE;
                }
            };
            match preview_pipeline::enqueue(&pool, &options.file_id, &options.profile).await {
                Ok(request) => {
                    println!("preview_id={}", request.id);
                    println!("state={}", request.status);
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("preview enqueue failed: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "process-next-preview" => {
            let worker_config = match preview_pipeline::PreviewWorkerConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    runtime_log::failure(
                        "preview-worker",
                        "preview_worker.start_failed",
                        "worker_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let database_config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    runtime_log::failure(
                        "preview-worker",
                        "preview_worker.start_failed",
                        "database_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&database_config).await {
                Ok(pool) => pool,
                Err(message) => {
                    runtime_log::failure(
                        "preview-worker",
                        "preview_worker.start_failed",
                        "database_connect_failed",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let result = preview_pipeline::process_batch(&pool, &worker_config).await;
            if let Err(message) =
                operations::record_component(&pool, "preview-worker", result.is_ok()).await
            {
                runtime_log::failure(
                    "preview-worker",
                    "preview_worker.heartbeat_failed",
                    "heartbeat_failed",
                    &message,
                );
                return ExitCode::FAILURE;
            }
            match result {
                Ok(reports) if !reports.is_empty() => {
                    for report in &reports {
                        runtime_log::record(
                            &pool,
                            "preview-worker",
                            "preview_worker.job.completed",
                            "preview_job_terminal",
                            runtime_log::Severity::Info,
                            &format!("state={}", report.status),
                            Some(&report.id),
                        )
                        .await;
                    }
                    ExitCode::SUCCESS
                }
                Ok(_) => {
                    runtime_log::record(
                        &pool,
                        "preview-worker",
                        "preview_worker.idle",
                        "queue_empty",
                        runtime_log::Severity::Debug,
                        "no queued preview job",
                        None,
                    )
                    .await;
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    runtime_log::record(
                        &pool,
                        "preview-worker",
                        "preview_worker.failed",
                        "preview_worker_failed",
                        runtime_log::Severity::Error,
                        &message,
                        None,
                    )
                    .await;
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "process-next-scan" => {
            let database_config = match database::DatabaseConfig::from_environment() {
                Ok(config) => config,
                Err(message) => {
                    runtime_log::failure(
                        "scan-worker",
                        "scan_worker.start_failed",
                        "database_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&database_config).await {
                Ok(pool) => pool,
                Err(message) => {
                    runtime_log::failure(
                        "scan-worker",
                        "scan_worker.start_failed",
                        "database_connect_failed",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let result = scan_pipeline::process_batch(&pool).await;
            if let Err(message) =
                operations::record_component(&pool, "scan-worker", result.is_ok()).await
            {
                runtime_log::failure(
                    "scan-worker",
                    "scan_worker.heartbeat_failed",
                    "heartbeat_failed",
                    &message,
                );
                return ExitCode::FAILURE;
            }
            match result {
                Ok(reports) if !reports.is_empty() => {
                    for report in &reports {
                        runtime_log::record(
                            &pool,
                            "scan-worker",
                            "scan_worker.job.completed",
                            "scan_job_terminal",
                            runtime_log::Severity::Info,
                            &format!("state={}", report.status),
                            Some(&report.id),
                        )
                        .await;
                    }
                    ExitCode::SUCCESS
                }
                Ok(_) => {
                    runtime_log::record(
                        &pool,
                        "scan-worker",
                        "scan_worker.idle",
                        "queue_empty",
                        runtime_log::Severity::Debug,
                        "no queued scan job",
                        None,
                    )
                    .await;
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    runtime_log::record(
                        &pool,
                        "scan-worker",
                        "scan_worker.failed",
                        "scan_worker_failed",
                        runtime_log::Severity::Error,
                        &message,
                        None,
                    )
                    .await;
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "process-schedules" => {
            let config = match database::DatabaseConfig::from_environment() {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "scheduler",
                        "scheduler.start_failed",
                        "database_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "scheduler",
                        "scheduler.start_failed",
                        "database_connect_failed",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            match policy_worker::run_scheduler(&pool).await {
                Ok(count) => {
                    runtime_log::record(
                        &pool,
                        "scheduler",
                        "scheduler.completed",
                        "scheduler_ok",
                        runtime_log::Severity::Info,
                        &format!("processed={count}"),
                        None,
                    )
                    .await;
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    runtime_log::record(
                        &pool,
                        "scheduler",
                        "scheduler.failed",
                        "scheduler_failed",
                        runtime_log::Severity::Error,
                        &message,
                        None,
                    )
                    .await;
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "run-retention" => {
            let config = match database::DatabaseConfig::from_environment() {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "retention",
                        "retention.start_failed",
                        "database_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "retention",
                        "retention.start_failed",
                        "database_connect_failed",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let root = env::var_os("VOLUND_DERIVED_ROOT").map_or_else(
                || std::path::PathBuf::from("/srv/volund/derived"),
                std::path::PathBuf::from,
            );
            match policy_worker::run_retention(&pool, root).await {
                Ok(result) => {
                    runtime_log::record(
                        &pool,
                        "retention",
                        "retention.completed",
                        "retention_ok",
                        runtime_log::Severity::Info,
                        &format!("state={}", result.status),
                        Some(&result.id),
                    )
                    .await;
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    runtime_log::record(
                        &pool,
                        "retention",
                        "retention.failed",
                        "retention_failed",
                        runtime_log::Severity::Error,
                        &message,
                        None,
                    )
                    .await;
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "run-import-cleanup" => {
            let config = match database::DatabaseConfig::from_environment() {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "import-cleanup",
                        "import_cleanup.start_failed",
                        "database_config_invalid",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let pool = match database::connect(&config).await {
                Ok(value) => value,
                Err(message) => {
                    runtime_log::failure(
                        "import-cleanup",
                        "import_cleanup.start_failed",
                        "database_connect_failed",
                        &message,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let root = env::var_os("VOLUND_INCOMING_ROOT").map_or_else(
                || std::path::PathBuf::from("/srv/volund/incoming"),
                std::path::PathBuf::from,
            );
            let result = import_cleanup::run(&pool, &root).await;
            let heartbeat =
                operations::record_component(&pool, "import-cleanup", result.is_ok()).await;
            if let Err(message) = heartbeat {
                runtime_log::failure(
                    "import-cleanup",
                    "import_cleanup.heartbeat_failed",
                    "heartbeat_failed",
                    &message,
                );
                return ExitCode::FAILURE;
            }
            match result {
                Ok(value) => {
                    runtime_log::record(
                        &pool,
                        "import-cleanup",
                        "import_cleanup.completed",
                        "import_cleanup_ok",
                        runtime_log::Severity::Info,
                        &format!(
                            "state={} expired={} cleaned={} bytes={}",
                            value.status,
                            value.expired_drafts,
                            value.cleaned_drafts,
                            value.cleaned_bytes
                        ),
                        Some(&value.id),
                    )
                    .await;
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    runtime_log::record(
                        &pool,
                        "import-cleanup",
                        "import_cleanup.failed",
                        "import_cleanup_failed",
                        runtime_log::Severity::Error,
                        &message,
                        None,
                    )
                    .await;
                    ExitCode::FAILURE
                }
            }
        }
        Some(command) if command == "convert" => match job_runner::run_convert(arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("job failed: {message}");
                ExitCode::FAILURE
            }
        },
        Some(command) if command == "_execute-job" => match job_runner::execute_job(arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("job execution failed: {message}");
                ExitCode::FAILURE
            }
        },
        _ => {
            job_runner::print_usage();
            ExitCode::from(2)
        }
    }
}
