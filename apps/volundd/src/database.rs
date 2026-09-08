use std::env;
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};

const DEFAULT_DATABASE: &str = "volund";
const DEFAULT_USER: &str = "volund";
const DEFAULT_SOCKET_DIRECTORY: &str = "/var/run/postgresql";
pub const EXPECTED_MIGRATIONS: i64 = 32;
pub const EXPECTED_SCHEMA_TABLES: i64 = 37;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Clone)]
pub struct DatabaseConfig {
    database_url: Option<String>,
}

impl DatabaseConfig {
    /// Load database settings without inventing or persisting credentials.
    ///
    /// `VOLUND_DATABASE_URL` is optional. Without it, VÖLUND connects to the
    /// local Unix socket as role and database `volund`, relying on peer auth.
    ///
    /// # Errors
    ///
    /// Returns an error when the environment variable exists but is empty.
    pub fn from_environment() -> Result<Self, String> {
        Self::from_optional_url(env::var("VOLUND_DATABASE_URL").ok())
    }

    /// Build explicit settings, primarily for isolated integration databases.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty URL.
    pub fn from_url(database_url: String) -> Result<Self, String> {
        Self::from_optional_url(Some(database_url))
    }

    fn from_optional_url(database_url: Option<String>) -> Result<Self, String> {
        if database_url.as_deref().is_some_and(str::is_empty) {
            return Err("VOLUND_DATABASE_URL must not be empty".to_owned());
        }
        Ok(Self { database_url })
    }

    fn connect_options(&self) -> Result<PgConnectOptions, String> {
        let options = match &self.database_url {
            Some(url) => PgConnectOptions::from_str(url)
                .map_err(|error| format!("invalid VOLUND_DATABASE_URL: {error}"))?,
            None => PgConnectOptions::new()
                .host(DEFAULT_SOCKET_DIRECTORY)
                .username(DEFAULT_USER)
                .database(DEFAULT_DATABASE),
        };
        Ok(options.application_name("volundd"))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DatabaseHealth {
    pub database: String,
    pub role: String,
    pub server_version_num: i32,
    pub schema_exists: bool,
    pub schema_table_count: i64,
    pub applied_migrations: i64,
}

impl DatabaseHealth {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.server_version_num >= 170_000
            && self.schema_exists
            && self.schema_table_count == EXPECTED_SCHEMA_TABLES
            && self.applied_migrations == EXPECTED_MIGRATIONS
    }
}

/// Open a bounded `PostgreSQL` connection pool.
///
/// # Errors
///
/// Returns an error for invalid settings or an unavailable database.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, String> {
    let options = config.connect_options()?;
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                sqlx::query("SET search_path TO public, volund")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .map_err(|error| format!("cannot connect to PostgreSQL: {error}"))
}

/// Apply all pending embedded migrations and return the number newly applied.
///
/// # Errors
///
/// Returns an error when migration history is incompatible or SQL execution
/// fails. Committed migration files are never modified by this operation.
pub async fn run_migrations(pool: &PgPool) -> Result<i64, String> {
    let before = applied_migration_count(pool).await?;
    MIGRATOR
        .run(pool)
        .await
        .map_err(|error| format!("database migration failed: {error}"))?;
    let after = applied_migration_count(pool).await?;
    Ok(after - before)
}

/// Inspect `PostgreSQL` and VÖLUND schema health without changing database state.
///
/// # Errors
///
/// Returns an error when health queries cannot be completed.
pub async fn inspect(pool: &PgPool) -> Result<DatabaseHealth, String> {
    let row = sqlx::query(
        "SELECT current_database(), current_user, \
         current_setting('server_version_num')::integer, \
         to_regnamespace('volund') IS NOT NULL, \
         (SELECT count(*)::bigint FROM information_schema.tables \
          WHERE table_schema = 'volund' AND table_type = 'BASE TABLE')",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot inspect PostgreSQL: {error}"))?;

    Ok(DatabaseHealth {
        database: row.get(0),
        role: row.get(1),
        server_version_num: row.get(2),
        schema_exists: row.get(3),
        schema_table_count: row.get(4),
        applied_migrations: applied_migration_count(pool).await?,
    })
}

async fn applied_migration_count(pool: &PgPool) -> Result<i64, String> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(pool)
            .await
            .map_err(|error| format!("cannot inspect migration history: {error}"))?;
    if !exists {
        return Ok(0);
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint FROM public._sqlx_migrations WHERE success",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count applied migrations: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_explicit_database_url_is_rejected() {
        assert!(DatabaseConfig::from_url(String::new()).is_err());
    }

    #[test]
    fn valid_explicit_database_url_builds_connect_options() {
        let config =
            DatabaseConfig::from_url("postgresql://volund@localhost/volund_test".to_owned())
                .expect("valid config");
        assert!(config.connect_options().is_ok());
    }

    #[test]
    fn health_requires_version_schema_tables_and_migrations() {
        let ready = DatabaseHealth {
            database: "volund".to_owned(),
            role: "volund".to_owned(),
            server_version_num: 170_011,
            schema_exists: true,
            schema_table_count: EXPECTED_SCHEMA_TABLES,
            applied_migrations: EXPECTED_MIGRATIONS,
        };
        assert!(ready.is_ready());
        assert!(
            !DatabaseHealth {
                schema_exists: false,
                ..ready
            }
            .is_ready()
        );
    }
}
