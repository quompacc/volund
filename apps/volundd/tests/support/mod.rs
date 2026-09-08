#![cfg(target_os = "linux")]

use std::env;
use std::ops::Deref;

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use serde_json::Value;
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
use volundd::database::{self, DatabaseConfig};
use volundd::identity::{BootstrapSecret, FirstOwnerInput, create_first_owner};
use volundd::session::{LoginInput, SessionConfig, login};

const TEST_LOCK_ID: i64 = 8_607_563_683_953_084_228;

/// Send an unauthenticated or pre-authenticated GET and decode its JSON body.
#[allow(dead_code)] // Each integration-test crate compiles this shared module independently.
pub async fn get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("parse JSON response"),
    )
}

/// A `PostgreSQL` pool whose fixture state is isolated from every other test.
///
/// The dedicated connection owns a cross-process advisory lock. Dropping the
/// guard closes that connection and releases the lock even when a test panics.
pub struct TestDatabase {
    pool: PgPool,
    _lock: PgConnection,
}

impl Deref for TestDatabase {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

/// Connect to, validate, migrate, lock, and reset the disposable test database.
///
/// Tests skip when `VOLUND_TEST_DATABASE_URL` is absent. The database name must
/// contain `test` or `audit`, preventing an accidental reset of production.
pub async fn test_database() -> Option<TestDatabase> {
    let Ok(database_url) = env::var("VOLUND_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL test: VOLUND_TEST_DATABASE_URL is not set");
        return None;
    };
    let config = DatabaseConfig::from_url(database_url.clone()).expect("valid test database URL");
    let pool = database::connect(&config)
        .await
        .expect("connect to isolated test database");
    let database_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("read test database name");
    assert!(
        database_name.to_ascii_lowercase().contains("test")
            || database_name.to_ascii_lowercase().contains("audit"),
        "VOLUND_TEST_DATABASE_URL must name a disposable test or audit database"
    );
    database::run_migrations(&pool)
        .await
        .expect("apply test migrations");
    let mut lock = PgConnection::connect(&database_url)
        .await
        .expect("connect test isolation lock");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(TEST_LOCK_ID)
        .execute(&mut lock)
        .await
        .expect("lock PostgreSQL integration fixture");
    sqlx::query(
        "DO $reset$ DECLARE names text; BEGIN \
         SELECT string_agg(format('%I.%I', schemaname, tablename), ', ') INTO names \
         FROM pg_tables WHERE schemaname = 'volund'; \
         IF names IS NOT NULL THEN EXECUTE 'TRUNCATE TABLE ' || names || \
         ' RESTART IDENTITY CASCADE'; END IF; END $reset$",
    )
    .execute(&pool)
    .await
    .expect("reset isolated test data");
    sqlx::query(
        "INSERT INTO volund.instance_state (singleton) VALUES (true) \
         ON CONFLICT (singleton) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("restore singleton instance state");
    sqlx::query(
        "INSERT INTO volund.conversion_profiles (name,native_preset,built_in) VALUES ('web','web',true),('fine','fine',true) ON CONFLICT (name) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("restore built-in conversion profiles");
    Some(TestDatabase { pool, _lock: lock })
}

/// Add a real owner session to requests made by catalog integration tests.
#[allow(dead_code)] // Each integration-test crate compiles this shared module independently.
pub async fn authenticated_router(pool: &PgPool, router: axum::Router) -> axum::Router {
    const BOOTSTRAP: &str = "catalog-test-bootstrap-token-with-entropy";
    const PASSWORD: &str = "catalog test password with enough entropy";
    let secret = BootstrapSecret::from_token(BOOTSTRAP).expect("test bootstrap secret");
    create_first_owner(
        pool,
        &secret,
        BOOTSTRAP,
        FirstOwnerInput {
            email: "catalog-owner@example.test".to_owned(),
            display_name: "Catalog Test Owner".to_owned(),
            password: PASSWORD.to_owned(),
        },
    )
    .await
    .expect("create catalog test owner");
    let logged_in = login(
        pool,
        LoginInput {
            email: "catalog-owner@example.test".to_owned(),
            password: PASSWORD.to_owned(),
            client_address: None,
            user_agent: Some("volund-integration-test".to_owned()),
        },
        SessionConfig::default().policy(),
    )
    .await
    .expect("create catalog test session");
    let cookie = HeaderValue::from_str(&format!("volund_session={}", logged_in.session_token))
        .expect("test session cookie");
    let csrf = HeaderValue::from_str(&logged_in.csrf_token).expect("test CSRF token");
    router.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let cookie = cookie.clone();
            let csrf = csrf.clone();
            async move {
                request.headers_mut().insert(header::COOKIE, cookie);
                request.headers_mut().insert("x-csrf-token", csrf);
                next.run(request).await
            }
        },
    ))
}
