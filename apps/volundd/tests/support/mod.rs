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

pub mod conversion_worker;

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

/// Check `value` against the subset of JSON Schema used by VÖLUND contracts:
/// `type`, `const`, `enum`, `pattern` (anchored hex only), `required`,
/// `properties`, `additionalProperties: false` and `items`.
#[allow(dead_code)] // Each integration-test crate compiles this shared module independently.
pub fn assert_matches_schema(schema: &Value, value: &Value, path: &str) {
    if let Some(expected) = schema.get("const") {
        assert_eq!(value, expected, "{path}: const");
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        assert!(options.contains(value), "{path}: {value} not in enum");
    }
    if let Some(kind) = schema.get("type") {
        let kinds: Vec<&str> = match kind {
            Value::String(single) => vec![single.as_str()],
            Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let matches = kinds.iter().any(|kind| match *kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        });
        assert!(matches, "{path}: {value} is not {kinds:?}");
    }
    if schema.get("pattern").and_then(Value::as_str) == Some("^[a-f0-9]{64}$") {
        if let Some(text) = value.as_str() {
            assert!(
                text.len() == 64 && text.chars().all(|c| matches!(c, 'a'..='f' | '0'..='9')),
                "{path}: not a SHA-256"
            );
        }
    }
    if let Some(object) = value.as_object() {
        for key in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = key.as_str().expect("required key");
            assert!(object.contains_key(key), "{path}: missing {key}");
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, child) in object {
            match properties.and_then(|properties| properties.get(key)) {
                Some(child_schema) => {
                    assert_matches_schema(child_schema, child, &format!("{path}.{key}"));
                }
                None => assert!(
                    schema.get("additionalProperties") != Some(&Value::Bool(false)),
                    "{path}: unexpected property {key}"
                ),
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            assert_matches_schema(items, item, &format!("{path}[{index}]"));
        }
    }
}
