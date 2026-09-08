#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::session::{self, LoginInput, LoginSession, SessionPolicy};
const PASSWORD: &str = "isolated session ownership test password";

#[tokio::test]
async fn malformed_and_unknown_invitation_tokens_do_not_consume_a_valid_invitation() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let (status, invitation) = call(
        &owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"negative-token@example.test","displayName":"Token matrix","role":"viewer"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for token in [
        String::new(),
        "x".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        "a".repeat(64),
        "ü".repeat(32),
    ] {
        let response = call(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":token,"password":PASSWORD}),
            None,
        )
        .await;
        assert_eq!(response.0, StatusCode::NOT_FOUND);
        assert!(!response.1.to_string().contains(PASSWORD));
        let untouched: bool = sqlx::query_scalar("SELECT i.accepted_at IS NULL AND u.status='invited' AND NOT EXISTS(SELECT 1 FROM volund.password_credentials c WHERE c.user_id=u.id) FROM volund.user_invitations i JOIN volund.users u ON u.id=i.user_id")
            .fetch_one(&*db).await.unwrap();
        assert!(untouched);
    }
    assert_eq!(
        call(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":invitation["activationToken"],"password":PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='user.invitation_accept' AND outcome='success'")
        .fetch_one(&*db).await.unwrap();
    assert_eq!(count, 1);
}
async fn call(
    router: &Router,
    method: &str,
    path: &str,
    value: Value,
    login: Option<&LoginSession>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(l) = login {
        req = req
            .header("cookie", format!("volund_session={}", l.session_token))
            .header("x-csrf-token", &l.csrf_token);
    }
    let response = router
        .clone()
        .oneshot(req.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let b = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        if b.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&b).unwrap()
        },
    )
}
async fn login(db: &sqlx::PgPool, email: &str) -> LoginSession {
    session::login(
        db,
        LoginInput {
            email: email.to_owned(),
            password: PASSWORD.to_owned(),
            client_address: None,
            user_agent: None,
        },
        SessionPolicy::default(),
    )
    .await
    .unwrap()
}
async fn user(
    db: &sqlx::PgPool,
    base: &Router,
    owner: &Router,
    email: &str,
    role: &str,
) -> LoginSession {
    let (s, i) = call(
        owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":email,"displayName":email,"role":role}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        call(
            base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":i["activationToken"],"password":PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    login(db, email).await
}

#[tokio::test]
async fn repeated_reset_revokes_old_sessions_and_only_latest_secret_works() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let original = user(&db, &base, &owner, "reset@example.test", "editor").await;
    let path = format!("/api/v1/users/{}/password", original.session.user_id);
    let first = "first isolated temporary credential";
    let second = "second isolated temporary credential";
    for password in [first, second] {
        assert_eq!(
            call(&owner, "POST", &path, json!({"password":password}), None).await,
            (StatusCode::NO_CONTENT, Value::Null)
        );
        assert_eq!(
            call(&base, "GET", "/api/v1/session", json!({}), Some(&original))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    for password in [PASSWORD, first] {
        assert_eq!(
            call(
                &base,
                "POST",
                "/api/v1/sessions",
                json!({"email":"reset@example.test","password":password}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let temporary = session::login(
        &db,
        LoginInput {
            email: "reset@example.test".into(),
            password: second.into(),
            client_address: None,
            user_agent: None,
        },
        SessionPolicy::default(),
    )
    .await
    .unwrap();
    assert!(temporary.session.must_change_password);
    for path in ["/api/v1/models", "/api/v1/users", "/api/v1/settings"] {
        assert_eq!(
            call(&base, "GET", path, json!({}), Some(&temporary))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(
            &base,
            "PUT",
            "/api/v1/account/password",
            json!({"currentPassword":first,"newPassword":PASSWORD}),
            Some(&temporary)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &base,
            "PUT",
            "/api/v1/account/password",
            json!({"currentPassword":second,"newPassword":PASSWORD}),
            Some(&temporary)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&base, "GET", "/api/v1/models", json!({}), Some(&temporary))
            .await
            .0,
        StatusCode::OK
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action='user.password_reset'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(audits, 2);
}

#[tokio::test]
async fn credential_secrets_are_absent_from_inventory_and_stored_logs() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let (status, invitation) = call(
        &owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"secret@example.test","displayName":"Secret test","role":"viewer"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = invitation["activationToken"].as_str().unwrap();
    assert_eq!(
        call(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":token,"password":PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let actor = login(&db, "secret@example.test").await;
    let reset = "distinct secret for reset verification";
    let path = format!("/api/v1/users/{}/password", actor.session.user_id);
    let response = call(&owner, "POST", &path, json!({"password":reset}), None).await;
    assert_eq!(response, (StatusCode::NO_CONTENT, Value::Null));
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM volund.password_credentials WHERE user_id=(SELECT id FROM volund.users WHERE email='secret@example.test')").fetch_one(&*db).await.unwrap();
    assert!(hash.starts_with("$argon2id$"));
    let (_, inventory) = call(&owner, "GET", "/api/v1/users", json!({}), None).await;
    let logs: String = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(e))::text,'[]') FROM volund.operational_log_events e",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    let audits: String = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(e))::text,'[]') FROM volund.security_audit_events e",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert!(logs.contains("http.request.completed"));
    assert!(audits.contains("user.password_reset"));
    for output in [inventory.to_string(), logs, audits] {
        for secret in [
            PASSWORD,
            reset,
            token,
            &actor.session_token,
            &actor.csrf_token,
            &hash,
        ] {
            assert!(
                !output.contains(secret),
                "secret leaked into inventory or stored logs"
            );
        }
    }
}

#[tokio::test]
async fn administrator_cannot_reset_owner_or_invalidate_owner_session() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let admin = user(
        &db,
        &base,
        &owner,
        "admin-reset@example.test",
        "administrator",
    )
    .await;
    let (_, current) = call(&owner, "GET", "/api/v1/session", json!({}), None).await;
    let path = format!(
        "/api/v1/users/{}/password",
        current["userId"].as_str().unwrap()
    );
    let before: String = sqlx::query_scalar("SELECT password_hash FROM volund.password_credentials WHERE user_id=(SELECT id FROM volund.users WHERE role='owner')").fetch_one(&*db).await.unwrap();
    assert_eq!(
        call(
            &base,
            "POST",
            &path,
            json!({"password":"forbidden owner replacement secret","mustChange":false}),
            Some(&admin)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let after: String = sqlx::query_scalar("SELECT password_hash FROM volund.password_credentials WHERE user_id=(SELECT id FROM volund.users WHERE role='owner')").fetch_one(&*db).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(
        call(&owner, "GET", "/api/v1/users", json!({}), None)
            .await
            .0,
        StatusCode::OK
    );
    let resets: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action='user.password_reset'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(resets, 0);
}
