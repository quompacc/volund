#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api;
use volundd::identity::{
    BootstrapSecret, FirstOwnerInput, IdentityConfig, IdentityError, create_first_owner,
    setup_complete, verify_password,
};

mod support;

use support::test_database;

#[tokio::test]
async fn first_owner_setup_is_atomic_hashed_and_audited() {
    let Some(pool) = test_database().await else {
        return;
    };
    let token = "phase-one-bootstrap-token-with-enough-entropy";
    let secret = BootstrapSecret::from_token(token).expect("bootstrap secret");
    assert!(!setup_complete(&pool).await.expect("initial setup state"));

    let denied = create_first_owner(
        &pool,
        &secret,
        "wrong-token-with-enough-entropy-000",
        owner_input(),
    )
    .await;
    assert_eq!(denied, Err(IdentityError::InvalidBootstrapToken));
    let owner = create_first_owner(&pool, &secret, token, owner_input())
        .await
        .expect("create first owner");
    assert_eq!(owner.email, "Owner@Example.test");
    assert_eq!(owner.role, "owner");
    assert!(setup_complete(&pool).await.expect("completed setup state"));

    let password_hash: String = sqlx::query_scalar(
        "SELECT credential.password_hash FROM volund.password_credentials credential \
         JOIN volund.users account ON account.id = credential.user_id \
         WHERE account.public_id::text = $1",
    )
    .bind(&owner.id)
    .fetch_one(&*pool)
    .await
    .expect("load password hash");
    assert_ne!(password_hash, "correct horse battery staple");
    assert!(verify_password(
        "correct horse battery staple",
        &password_hash
    ));

    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT outcome FROM volund.security_audit_events \
         WHERE action = 'setup.first_owner' ORDER BY id",
    )
    .fetch_all(&*pool)
    .await
    .expect("load setup audit");
    assert_eq!(outcomes, ["denied", "success"]);

    let duplicate = create_first_owner(&pool, &secret, token, owner_input()).await;
    assert_eq!(duplicate, Err(IdentityError::AlreadyInitialized));
}

fn owner_input() -> FirstOwnerInput {
    FirstOwnerInput {
        email: " Owner@Example.test ".to_owned(),
        display_name: " First Owner ".to_owned(),
        password: "correct horse battery staple".to_owned(),
    }
}

#[tokio::test]
async fn setup_api_requires_configured_credentials_and_locks_after_success() {
    let Some(pool) = test_database().await else {
        return;
    };
    let token = "api-bootstrap-token-with-enough-random-material";
    let secret = BootstrapSecret::from_token(token).expect("bootstrap secret");
    let router = api::router_with_identity(
        (*pool).clone(),
        "/tmp/volund-identity-derived".into(),
        "/tmp/volund-identity-web".into(),
        IdentityConfig::with_bootstrap_secret(secret),
    );

    let status = router
        .clone()
        .oneshot(request("GET", "/api/v1/setup", None, None))
        .await
        .expect("setup status response");
    assert_eq!(status.status(), StatusCode::OK);
    let status = response_json(status).await;
    assert_eq!(status["initialized"], false);
    assert_eq!(status["bootstrapAvailable"], true);

    let denied = router
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/setup/owner",
            Some(owner_json()),
            None,
        ))
        .await
        .expect("denied setup response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let created = router
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/setup/owner",
            Some(owner_json()),
            Some(token),
        ))
        .await
        .expect("created owner response");
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(response_json(created).await["role"], "owner");

    let duplicate = router
        .oneshot(request(
            "POST",
            "/api/v1/setup/owner",
            Some(owner_json()),
            Some(token),
        ))
        .await
        .expect("duplicate setup response");
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
}

fn owner_json() -> Value {
    json!({
        "email": "owner@example.test",
        "displayName": "First Owner",
        "password": "correct horse battery staple"
    })
}

fn request(method: &str, uri: &str, body: Option<Value>, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(token) = token {
        builder = builder.header("x-volund-setup-token", token);
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("build setup request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read setup response body");
    serde_json::from_slice(&body).expect("parse setup response")
}
