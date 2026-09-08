#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api;

mod support;

use support::{authenticated_router, test_database};

#[tokio::test]
async fn owner_manages_users_credentials_and_access_atomically() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = api::router_with_roots(
        (*pool).clone(),
        "/tmp/volund-users-derived".into(),
        "/tmp/volund-users-web".into(),
    );
    let owner_router = authenticated_router(&pool, base.clone()).await;
    let (status, users) = request_json(&owner_router, "GET", "/api/v1/users", None).await;
    assert_eq!(status, StatusCode::OK);
    let owner_id = users[0]["id"].as_str().expect("owner ID");

    let (status, created) = request_json(
        &owner_router,
        "POST",
        "/api/v1/users",
        Some(json!({
            "email": "editor@example.test",
            "displayName": "Initial Editor",
            "role": "editor",
            "password": "initial editor password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let user_id = created["id"].as_str().expect("created user ID");
    let old_cookie = login_cookie(&base, "initial editor password").await;

    let (status, _) = request_json(
        &owner_router,
        "POST",
        &format!("/api/v1/users/{user_id}/password"),
        Some(json!({"password": "replacement editor password"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        current_status(&base, &old_cookie).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_status(&base, "initial editor password").await,
        StatusCode::UNAUTHORIZED
    );
    let replacement_cookie = login_cookie(&base, "replacement editor password").await;

    let (status, updated) = request_json(
        &owner_router,
        "PATCH",
        &format!("/api/v1/users/{user_id}"),
        Some(json!({"displayName": "Disabled Viewer", "role": "viewer", "status": "disabled"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["role"], "viewer");
    assert_eq!(updated["status"], "disabled");
    assert_eq!(
        current_status(&base, &replacement_cookie).await,
        StatusCode::UNAUTHORIZED
    );

    let (status, error) = request_json(
        &owner_router,
        "PATCH",
        &format!("/api/v1/users/{owner_id}"),
        Some(json!({"status": "disabled"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "forbidden");
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action IN ('user.create', 'user.password_reset', 'user.update')",
    )
    .fetch_one(&*pool)
    .await
    .expect("count user administration audit events");
    assert_eq!(audit_count, 3);
}

async fn request_json(
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
            .expect("build JSON request")
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("build empty request")
    };
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("user API response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read user API response");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON user API response")
    };
    (status, value)
}

async fn login_status(router: &axum::Router, password: &str) -> StatusCode {
    login_response(router, password).await.status()
}

async fn login_cookie(router: &axum::Router, password: &str) -> String {
    let response = login_response(router, password).await;
    assert_eq!(response.status(), StatusCode::OK);
    let token = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            value
                .to_str()
                .ok()?
                .strip_prefix("volund_session=")?
                .split(';')
                .next()
        })
        .expect("session cookie");
    format!("volund_session={token}")
}

async fn login_response(router: &axum::Router, password: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"email": "editor@example.test", "password": password}).to_string(),
                ))
                .expect("build login request"),
        )
        .await
        .expect("login response")
}

async fn current_status(router: &axum::Router, cookie: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/session")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .expect("build current-session request"),
        )
        .await
        .expect("current-session response")
        .status()
}
