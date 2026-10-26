#![cfg(target_os = "linux")]
//! Personal API tokens (ADR 0037): shown once, hashed, scoped, expiring,
//! revocable, and locked out of account and session management.
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, test_database};
use tower::ServiceExt;

async fn call(
    router: &axum::Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if body.is_some() {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One account walks through the whole token lifecycle.
async fn api_tokens_are_scoped_expiring_revocable_and_browser_only_for_management() {
    let Some(pool) = test_database().await else {
        return;
    };
    let browser = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;
    // A router without the test session layer: only the bearer token authenticates.
    let api = volundd::api::router((*pool).clone());

    let (status, read) = call(
        &browser,
        Method::POST,
        "/api/v1/account/tokens",
        None,
        Some(json!({"name": "Backup-Skript", "scope": "read", "expiresInDays": 30})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{read}");
    let read_token = read["token"].as_str().unwrap().to_owned();
    assert!(read_token.starts_with("vlnd_") && read_token.len() == 69);
    assert_eq!(read["prefix"].as_str(), Some(&read_token[5..13]));
    let stored: (String, String) =
        sqlx::query_as("SELECT token_digest, token_prefix FROM volund.api_tokens")
            .fetch_one(&*pool)
            .await
            .unwrap();
    assert!(
        !stored.0.contains(&read_token[5..]),
        "only a digest is stored"
    );
    let (_, listed) = call(&browser, Method::GET, "/api/v1/account/tokens", None, None).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(
        listed[0].get("token").is_none(),
        "the secret is shown only once"
    );

    // A read token reads the catalog, cannot write, and cannot manage accounts.
    assert_eq!(
        call(&api, Method::GET, "/api/v1/models", Some(&read_token), None)
            .await
            .0,
        StatusCode::OK
    );
    let model = json!({"slug": "token-model", "name": "Token Model", "kind": "part", "primaryFileId": null});
    assert_eq!(
        call(
            &api,
            Method::POST,
            "/api/v1/models",
            Some(&read_token),
            Some(model.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for uri in [
        "/api/v1/account/tokens",
        "/api/v1/session",
        "/api/v1/sessions",
        "/api/v1/users",
    ] {
        assert_eq!(
            call(&api, Method::GET, uri, Some(&read_token), None)
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{uri}"
        );
    }
    assert_eq!(
        call(&api, Method::GET, "/api/v1/jobs", Some(&read_token), None)
            .await
            .0,
        StatusCode::FORBIDDEN,
        "tokens never carry administration capabilities"
    );
    let used: bool = sqlx::query_scalar("SELECT last_used_at IS NOT NULL FROM volund.api_tokens")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert!(used);

    // A catalog-write token mutates without browser CSRF, within the owner's role.
    let (_, write) = call(
        &browser,
        Method::POST,
        "/api/v1/account/tokens",
        None,
        Some(json!({"name": "Import", "scope": "catalog-write"})),
    )
    .await;
    let write_token = write["token"].as_str().unwrap().to_owned();
    let (status, created) = call(
        &api,
        Method::POST,
        "/api/v1/models",
        Some(&write_token),
        Some(model),
    )
    .await;
    assert_ne!(status, StatusCode::FORBIDDEN, "{created}");
    assert_ne!(status, StatusCode::UNAUTHORIZED, "{created}");
    assert_eq!(
        call(&api, Method::GET, "/api/v1/jobs", Some(&write_token), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // Malformed, revoked, expired, and disabled-account tokens are refused.
    for bad in ["vlnd_short", &format!("vlnd_{}", "0".repeat(64))] {
        assert_eq!(
            call(&api, Method::GET, "/api/v1/models", Some(bad), None)
                .await
                .0,
            StatusCode::UNAUTHORIZED,
            "{bad}"
        );
    }
    let (status, _) = call(
        &browser,
        Method::DELETE,
        &format!("/api/v1/account/tokens/{}", read["id"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&api, Method::GET, "/api/v1/models", Some(&read_token), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE volund.api_tokens SET created_at=now()-interval '2 days', expires_at=now()-interval '1 second' WHERE name='Import'")
        .execute(&*pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &api,
            Method::GET,
            "/api/v1/models",
            Some(&write_token),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, fresh) = call(
        &browser,
        Method::POST,
        "/api/v1/account/tokens",
        None,
        Some(json!({"name": "Fresh", "scope": "read"})),
    )
    .await;
    let fresh_token = fresh["token"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE volund.users SET status='disabled'")
        .execute(&*pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &api,
            Method::GET,
            "/api/v1/models",
            Some(&fresh_token),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE volund.users SET status='active'")
        .execute(&*pool)
        .await
        .unwrap();

    for invalid in [
        json!({"name": "", "scope": "read"}),
        json!({"name": "Admin", "scope": "admin"}),
        json!({"name": "Forever", "scope": "read", "expiresInDays": 400}),
        json!({"name": "Zero", "scope": "read", "expiresInDays": 0}),
    ] {
        assert_eq!(
            call(
                &browser,
                Method::POST,
                "/api/v1/account/tokens",
                None,
                Some(invalid.clone())
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
            "{invalid}"
        );
    }
    for index in 0..19 {
        let (status, _) = call(
            &browser,
            Method::POST,
            "/api/v1/account/tokens",
            None,
            Some(json!({"name": format!("Bulk {index}"), "scope": "read"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, limited) = call(
        &browser,
        Method::POST,
        "/api/v1/account/tokens",
        None,
        Some(json!({"name": "Too many", "scope": "read"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited["error"]["code"], "api_token_limit");
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action IN ('api-token.create','api-token.revoke')")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(
        audits, 23,
        "3 + 19 creations and one revocation are audited"
    );
}
