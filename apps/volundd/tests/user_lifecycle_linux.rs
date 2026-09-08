#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api;

mod support;

use support::{authenticated_router, test_database};

struct BrowserCredentials {
    cookie: String,
    csrf: String,
}

#[tokio::test]
async fn invited_account_metadata_can_change_without_activating_it() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = api::router((*pool).clone());
    let owner = authenticated_router(&pool, base.clone()).await;
    let invitation = invite(&owner, "role-change@example.test").await;
    let id = invitation["user"]["id"].as_str().unwrap();
    let (status, changed) = request_json(
        &owner,
        "PATCH",
        &format!("/api/v1/users/{id}"),
        Some(json!({"role":"editor","displayName":"Renamed invite"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed["role"], "editor");
    assert_eq!(changed["status"], "invited");
    assert_eq!(changed["displayName"], "Renamed invite");
    assert_eq!(
        request_json(
            &owner,
            "PATCH",
            &format!("/api/v1/users/{id}"),
            Some(json!({"status":"invented"})),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(request_json(&base, "POST", "/api/v1/invitations/accept",
        Some(json!({"token":invitation["activationToken"],"password":"invitation chosen password"})), None).await.0,
        StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn invitation_acceptance_replaces_a_previously_reset_credential() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = api::router((*pool).clone());
    let owner = authenticated_router(&pool, base.clone()).await;
    let invitation = invite(&owner, "reset-invite@example.test").await;
    let id = invitation["user"]["id"].as_str().unwrap();
    assert_eq!(
        request_json(
            &owner,
            "POST",
            &format!("/api/v1/users/{id}/password"),
            Some(json!({"password":"administrator temporary password","mustChange":true})),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        login(
            &base,
            "reset-invite@example.test",
            "administrator temporary password"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let activation =
        json!({"token":invitation["activationToken"],"password":"personal invitation password"});
    assert_eq!(
        request_json(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            Some(activation.clone()),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request_json(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            Some(activation),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        login(
            &base,
            "reset-invite@example.test",
            "administrator temporary password"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, credentials) = login(
        &base,
        "reset-invite@example.test",
        "personal invitation password",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, current) =
        request_json(&base, "GET", "/api/v1/session", None, credentials.as_ref()).await;
    assert_eq!(current["mustChangePassword"], false);
}

#[tokio::test]
async fn invitation_is_single_use_expiring_and_never_stored_in_plaintext() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = api::router((*pool).clone());
    let owner = authenticated_router(&pool, base.clone()).await;
    let invitation = invite(&owner, "invited@example.test").await;
    let token = invitation["activationToken"]
        .as_str()
        .expect("one-time activation token");
    assert_eq!(invitation["user"]["status"], "invited");
    let stored_digest: String =
        sqlx::query_scalar("SELECT token_digest FROM volund.user_invitations")
            .fetch_one(&*pool)
            .await
            .expect("stored invitation digest");
    assert_ne!(stored_digest, token);
    assert_eq!(stored_digest.len(), 64);

    let activation = json!({"token": token, "password": "invited personal password"});
    assert_eq!(
        request_json(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            Some(activation.clone()),
            None,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request_json(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            Some(activation),
            None,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        login(&base, "invited@example.test", "invited personal password")
            .await
            .0,
        StatusCode::OK
    );
    let invitation_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action IN ('user.invite', 'user.invitation_accept')",
    )
    .fetch_one(&*pool)
    .await
    .expect("invitation audit events");
    assert_eq!(invitation_audits, 2);

    let expired = invite(&owner, "expired@example.test").await;
    sqlx::query(
        "UPDATE volund.user_invitations SET created_at = now() - interval '2 days', \
         expires_at = now() - interval '1 day' \
         WHERE user_id = (SELECT id FROM volund.users WHERE normalized_email = $1)",
    )
    .bind("expired@example.test")
    .execute(&*pool)
    .await
    .expect("expire second invitation");
    assert_eq!(
        request_json(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            Some(json!({
                "token": expired["activationToken"],
                "password": "expired invitation password"
            })),
            None,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn temporary_password_lock_and_unlock_complete_the_account_lifecycle() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = api::router((*pool).clone());
    let owner = authenticated_router(&pool, base.clone()).await;
    let user_id = create_temporary_user(&owner).await;
    replace_temporary_password(&base).await;
    verify_lock_and_unlock(&base, &owner, &user_id).await;
    let lifecycle_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action IN ('user.password_change', 'user.update')",
    )
    .fetch_one(&*pool)
    .await
    .expect("password and access audit events");
    assert_eq!(lifecycle_audits, 3);
}

async fn create_temporary_user(owner: &axum::Router) -> String {
    let (_, created) = request_json(
        owner,
        "POST",
        "/api/v1/users",
        Some(json!({
            "email": "lifecycle@example.test",
            "displayName": "Lifecycle User",
            "role": "editor",
            "password": "temporary lifecycle password"
        })),
        None,
    )
    .await;
    assert_eq!(created["mustChangePassword"], true);
    created["id"].as_str().expect("created user ID").to_owned()
}

async fn replace_temporary_password(base: &axum::Router) {
    let (status, credentials) = login(
        base,
        "lifecycle@example.test",
        "temporary lifecycle password",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let credentials = credentials.expect("browser credentials");
    let (_, current) = request_json(base, "GET", "/api/v1/session", None, Some(&credentials)).await;
    assert_eq!(current["mustChangePassword"], true);
    assert_eq!(
        request_json(base, "GET", "/api/v1/models", None, Some(&credentials))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_json(
            base,
            "PUT",
            "/api/v1/account/password",
            Some(json!({
                "currentPassword": "wrong current password",
                "newPassword": "personal lifecycle password"
            })),
            Some(&credentials),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request_json(
            base,
            "PUT",
            "/api/v1/account/password",
            Some(json!({
                "currentPassword": "temporary lifecycle password",
                "newPassword": "personal lifecycle password"
            })),
            Some(&credentials),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, current) = request_json(base, "GET", "/api/v1/session", None, Some(&credentials)).await;
    assert_eq!(current["mustChangePassword"], false);
    assert_eq!(
        request_json(base, "GET", "/api/v1/models", None, Some(&credentials))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        login(
            base,
            "lifecycle@example.test",
            "temporary lifecycle password"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn verify_lock_and_unlock(base: &axum::Router, owner: &axum::Router, user_id: &str) {
    for _ in 0..5 {
        assert_eq!(
            login(base, "lifecycle@example.test", "wrong password")
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (_, users) = request_json(owner, "GET", "/api/v1/users", None, None).await;
    let locked = users
        .as_array()
        .expect("user inventory")
        .iter()
        .find(|user| user["id"] == user_id)
        .expect("locked user");
    assert_eq!(locked["status"], "locked");
    assert!(locked["lockedUntilUnixMs"].is_number());
    let (status, unlocked) = request_json(
        owner,
        "PATCH",
        &format!("/api/v1/users/{user_id}"),
        Some(json!({"status": "active"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unlocked["status"], "active");
    assert_eq!(unlocked["lockedUntilUnixMs"], Value::Null);
    assert_eq!(
        login(
            base,
            "lifecycle@example.test",
            "personal lifecycle password"
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, locked) = request_json(
        owner,
        "PATCH",
        &format!("/api/v1/users/{user_id}"),
        Some(json!({"status": "locked"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(locked["status"], "locked");
    assert_eq!(
        login(
            base,
            "lifecycle@example.test",
            "personal lifecycle password"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn invite(router: &axum::Router, email: &str) -> Value {
    let (status, invitation) = request_json(
        router,
        "POST",
        "/api/v1/users/invitations",
        Some(json!({"email": email, "displayName": "Invited User", "role": "viewer"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    invitation
}

async fn login(
    router: &axum::Router,
    email: &str,
    password: &str,
) -> (StatusCode, Option<BrowserCredentials>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"email": email, "password": password}).to_string(),
                ))
                .expect("login request"),
        )
        .await
        .expect("login response");
    let status = response.status();
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok()?.split(';').next())
        .collect();
    let session = cookies
        .iter()
        .find(|value| value.starts_with("volund_session="));
    let csrf = cookies
        .iter()
        .find(|value| value.starts_with("volund_csrf="));
    let credentials = session.zip(csrf).map(|(session, csrf)| BrowserCredentials {
        cookie: format!("{session}; {csrf}"),
        csrf: csrf.trim_start_matches("volund_csrf=").to_owned(),
    });
    (status, credentials)
}

async fn request_json(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    credentials: Option<&BrowserCredentials>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(credentials) = credentials {
        builder = builder
            .header(header::COOKIE, &credentials.cookie)
            .header("x-csrf-token", &credentials.csrf);
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .expect("JSON request"),
        )
        .await
        .expect("JSON response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON response body")
    };
    (status, value)
}
