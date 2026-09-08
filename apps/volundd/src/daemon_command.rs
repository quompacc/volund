use std::process::ExitCode;

use crate::{api, api_config, database, identity, runtime_log, session};

/// Start the native HTTP daemon after validating every runtime boundary.
#[allow(clippy::too_many_lines)] // Linear startup gate keeps failure order and exit behavior explicit.
pub async fn run() -> ExitCode {
    let api_config = match api_config::ApiConfig::from_environment() {
        Ok(config) => config,
        Err(message) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "api_config_invalid",
                &message,
            );
            return ExitCode::FAILURE;
        }
    };
    let database_config = match database::DatabaseConfig::from_environment() {
        Ok(config) => config,
        Err(message) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "database_config_invalid",
                &message,
            );
            return ExitCode::FAILURE;
        }
    };
    let identity_config = match identity::IdentityConfig::from_environment() {
        Ok(config) => config,
        Err(error) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "identity_config_invalid",
                &format!("{error:?}"),
            );
            return ExitCode::FAILURE;
        }
    };
    let session_config = match session::SessionConfig::from_environment(
        api_config.listen_address.ip().is_loopback(),
    ) {
        Ok(config) => config,
        Err(error) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "session_config_invalid",
                &format!("{error:?}"),
            );
            return ExitCode::FAILURE;
        }
    };
    let pool = match database::connect(&database_config).await {
        Ok(pool) => pool,
        Err(message) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "database_connect_failed",
                &message,
            );
            return ExitCode::FAILURE;
        }
    };
    match database::inspect(&pool).await {
        Ok(health) if health.is_ready() => {}
        Ok(_) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "migration_required",
                "database schema is not ready",
            );
            return ExitCode::FAILURE;
        }
        Err(message) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "database_inspection_failed",
                &message,
            );
            return ExitCode::FAILURE;
        }
    }
    let listener = match tokio::net::TcpListener::bind(api_config.listen_address).await {
        Ok(listener) => listener,
        Err(error) => {
            runtime_log::failure(
                "daemon",
                "daemon.start_failed",
                "http_bind_failed",
                &error.to_string(),
            );
            return ExitCode::FAILURE;
        }
    };
    let runtime_pool = pool.clone();
    runtime_log::record(
        &runtime_pool,
        "daemon",
        "daemon.started",
        "daemon_ready",
        runtime_log::Severity::Info,
        "HTTP service ready on configured listener",
        None,
    )
    .await;
    match axum::serve(
        listener,
        api::router_with_security(
            pool,
            api_config.derived_root,
            api_config.web_root,
            api_config.support_root,
            identity_config,
            session_config,
        ),
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            runtime_log::record(
                &runtime_pool,
                "daemon",
                "daemon.failed",
                "http_server_failed",
                runtime_log::Severity::Error,
                &error.to_string(),
                None,
            )
            .await;
            ExitCode::FAILURE
        }
    }
}
