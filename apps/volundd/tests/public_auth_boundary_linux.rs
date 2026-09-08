#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(
    router: &Router,
    path: &str,
    value: Value,
    origin: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "volund.example.test")
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

#[tokio::test]
async fn login_lock_expiry_recovers_and_invalid_credentials_stay_generic() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let _owner = support::authenticated_router(&db, base.clone()).await;
    let email = "catalog-owner@example.test";
    let correct = json!({"email":email,"password":"catalog test password with enough entropy"});
    let unknown = post(
        &base,
        "/api/v1/sessions",
        json!({"email":"absent@example.test","password":"incorrect password"}),
        None,
    )
    .await;
    assert_eq!(unknown.0, StatusCode::UNAUTHORIZED);
    for _ in 0..5 {
        assert_eq!(
            post(
                &base,
                "/api/v1/sessions",
                json!({"email":email,"password":"incorrect password"}),
                None
            )
            .await,
            unknown
        );
    }
    assert_eq!(
        post(&base, "/api/v1/sessions", correct.clone(), None).await,
        unknown
    );
    sqlx::query("UPDATE volund.users SET locked_until=now()-interval '1 second'")
        .execute(&*db)
        .await
        .unwrap();
    assert_eq!(
        post(&base, "/api/v1/sessions", correct, None).await.0,
        StatusCode::OK
    );
    let reset: bool = sqlx::query_scalar(
        "SELECT failed_login_count=0 AND locked_until IS NULL FROM volund.users",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert!(reset);
    assert_eq!(
        post(
            &base,
            "/api/v1/sessions",
            json!({"email":email,"password":"x".repeat(1025)}),
            None
        )
        .await,
        unknown
    );
}

#[tokio::test]
async fn public_mutations_reject_foreign_origins_before_side_effects() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let credentials = json!({"email":"catalog-owner@example.test","password":"catalog test password with enough entropy"});
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.sessions")
        .fetch_one(&*db)
        .await
        .unwrap();
    for origin in [
        "http://foreign.example.test",
        "null",
        "https://volund.example.test",
        "http://volund.example.test:81",
    ] {
        assert_eq!(
            post(&base, "/api/v1/sessions", credentials.clone(), Some(origin))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(&base, "/api/v1/setup/owner", json!({}), Some(origin))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.sessions")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert_eq!(before, after);
    for origin in [None, Some("http://volund.example.test")] {
        assert_eq!(
            post(&base, "/api/v1/sessions", credentials.clone(), origin)
                .await
                .0,
            StatusCode::OK
        );
    }
    let (status, invitation) = post(
        &owner,
        "/api/v1/users/invitations",
        json!({"email":"origin@example.test","displayName":"Origin","role":"viewer"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let activation =
        json!({"token":invitation["activationToken"],"password":"valid origin boundary password"});
    assert_eq!(
        post(
            &base,
            "/api/v1/invitations/accept",
            activation.clone(),
            Some("http://foreign.example.test")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let untouched: bool =
        sqlx::query_scalar("SELECT accepted_at IS NULL FROM volund.user_invitations")
            .fetch_one(&*db)
            .await
            .unwrap();
    assert!(untouched);
    assert_eq!(
        post(
            &base,
            "/api/v1/invitations/accept",
            activation,
            Some("http://volund.example.test")
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn concurrent_invitation_acceptance_has_exactly_one_winner() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let (status, invitation) = post(
        &owner,
        "/api/v1/users/invitations",
        json!({"email":"race@example.test","displayName":"Race","role":"viewer"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first =
        json!({"token":invitation["activationToken"],"password":"first invitation race password"});
    let second =
        json!({"token":invitation["activationToken"],"password":"second invitation race password"});
    let (a, b) = tokio::join!(
        post(&base, "/api/v1/invitations/accept", first.clone(), None),
        post(&base, "/api/v1/invitations/accept", second.clone(), None)
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::NO_CONTENT, StatusCode::NOT_FOUND)
            | (StatusCode::NOT_FOUND, StatusCode::NO_CONTENT)
    ));
    for (activation, result) in [(first, a.0), (second, b.0)] {
        let expected = if result == StatusCode::NO_CONTENT {
            StatusCode::OK
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(
            post(
                &base,
                "/api/v1/sessions",
                json!({"email":"race@example.test","password":activation["password"]}),
                None
            )
            .await
            .0,
            expected
        );
        assert_eq!(
            post(&base, "/api/v1/invitations/accept", activation, None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
}
