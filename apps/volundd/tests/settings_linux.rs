#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api;

mod support;

use support::{authenticated_router, test_database};

#[tokio::test]
async fn settings_registry_validates_persists_and_detects_stale_writes() {
    let Some(pool) = test_database().await else {
        return;
    };
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let (status, settings) = request(&router, "GET", "/api/v1/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_registry(&settings);
    verify_setting_updates(&router).await;
    verify_user_preferences(&router).await;
    verify_effective_session_policy(&pool).await;
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action = 'setting.update'",
    )
    .fetch_one(&*pool)
    .await
    .expect("count setting audit events");
    assert_eq!(audit_count, 2);
}

async fn verify_user_preferences(router: &axum::Router) {
    let (status, defaults) = request(router, "GET", "/api/v1/preferences", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(defaults["previewAutoLoad"], "selected");
    assert_eq!(defaults["revision"], 0);
    let update = json!({
        "previewAutoLoad":"manual", "background":"light", "gridVisible":false,
        "contrast":"high", "renderStyle":"wireframe", "problemMinimumSeverity":"error",
        "expectedRevision":0
    });
    let (status, saved) = request(router, "PUT", "/api/v1/preferences", Some(update.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["background"], "light");
    assert_eq!(saved["revision"], 1);
    let (status, conflict) = request(router, "PUT", "/api/v1/preferences", Some(update)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "revision_conflict");
    let (status, rejected) = request(
        router,
        "PUT",
        "/api/v1/preferences",
        Some(json!({
            "previewAutoLoad":"all", "background":"light", "gridVisible":false,
            "contrast":"high", "renderStyle":"wireframe", "problemMinimumSeverity":"error",
            "expectedRevision":1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(rejected["error"]["code"], "bad_request");
}

fn assert_registry(settings: &Value) {
    assert_eq!(settings.as_array().expect("settings registry").len(), 24);
    let instance_name = settings
        .as_array()
        .expect("settings registry")
        .iter()
        .find(|setting| setting["key"] == "instance.name")
        .expect("instance name setting");
    assert_eq!(instance_name["value"], "VÖLUND");
    assert_eq!(instance_name["origin"], "default");
    assert_eq!(instance_name["revision"], 0);
    assert_eq!(instance_name["editable"], true);
    assert_eq!(instance_name["sensitive"], false);
    let database_override = settings
        .as_array()
        .expect("settings registry")
        .iter()
        .find(|setting| setting["key"] == "database.connectionOverride")
        .expect("database override diagnostic");
    assert_eq!(database_override["value"], Value::Null);
    assert_eq!(database_override["editable"], false);
    assert_eq!(database_override["sensitive"], true);
    assert_eq!(database_override["configured"], false);
    assert_eq!(database_override["effect"], "restart");
    let idle_duration = settings
        .as_array()
        .expect("settings registry")
        .iter()
        .find(|setting| setting["key"] == "security.sessionIdleMinutes")
        .expect("session idle setting");
    assert_eq!(idle_duration["value"], 480);
    assert_eq!(idle_duration["valueType"], "integer");
    let import_limit = settings
        .as_array()
        .expect("settings registry")
        .iter()
        .find(|setting| setting["key"] == "limits.importMaxFiles")
        .expect("compiled import limit");
    assert_eq!(import_limit["value"], 10_000);
    assert_eq!(import_limit["editable"], false);
    let import_capacity = settings
        .as_array()
        .expect("settings registry")
        .iter()
        .find(|setting| setting["key"] == "imports.incomingCapacityBytes")
        .expect("managed import capacity");
    assert_eq!(import_capacity["value"], 20_i64 * 1024 * 1024 * 1024);
    assert_eq!(import_capacity["editable"], true);
}

async fn verify_setting_updates(router: &axum::Router) {
    let update = json!({"value": "Werkstatt", "expectedRevision": 0});
    let (status, updated) = request(
        router,
        "PUT",
        "/api/v1/settings/instance.name",
        Some(update.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["value"], "Werkstatt");
    assert_eq!(updated["origin"], "persisted");
    assert_eq!(updated["revision"], 1);
    let (status, conflict) = request(
        router,
        "PUT",
        "/api/v1/settings/instance.name",
        Some(update),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "revision_conflict");
    let (status, _) = request(
        router,
        "PUT",
        "/api/v1/settings/unknown.key",
        Some(json!({"value": true, "expectedRevision": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, operator_managed) = request(
        router,
        "PUT",
        "/api/v1/settings/runtime.listenAddress",
        Some(json!({"value": "0.0.0.0:8080", "expectedRevision": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(operator_managed["error"]["code"], "bad_request");
    let (status, invalid_integer) = request(
        router,
        "PUT",
        "/api/v1/settings/security.sessionIdleMinutes",
        Some(json!({"value": "480", "expectedRevision": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_integer["error"]["code"], "bad_request");
    let (status, session_duration) = request(
        router,
        "PUT",
        "/api/v1/settings/security.sessionIdleMinutes",
        Some(json!({"value": 30, "expectedRevision": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session_duration["value"], 30);
}

async fn verify_effective_session_policy(pool: &sqlx::PgPool) {
    let policy = volundd::settings::session_policy(pool)
        .await
        .expect("effective session policy");
    let logged_in = volundd::session::login(
        pool,
        volundd::session::LoginInput {
            email: "catalog-owner@example.test".to_owned(),
            password: "catalog test password with enough entropy".to_owned(),
            client_address: None,
            user_agent: None,
        },
        policy,
    )
    .await
    .expect("login with configured session duration");
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after Unix epoch")
            .as_millis(),
    )
    .expect("current timestamp fits in i64");
    let idle_window = logged_in.session.idle_expires_at_unix_ms - now;
    assert!((29 * 60 * 1000..=30 * 60 * 1000).contains(&idle_window));
}

async fn request(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = if let Some(value) = body {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .expect("build JSON settings request")
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("build settings request")
    };
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("settings response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read settings response");
    (
        status,
        serde_json::from_slice(&body).expect("JSON settings response"),
    )
}
