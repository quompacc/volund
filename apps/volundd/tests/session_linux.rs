#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api;
use volundd::identity::{BootstrapSecret, FirstOwnerInput, create_first_owner};
use volundd::session::{
    LoginInput, SessionError, SessionPolicy, authenticate, login, revoke_current,
};

mod support;

use support::test_database;

#[tokio::test]
async fn login_uses_opaque_digests_and_revocation_is_immediate() {
    let Some(pool) = test_database().await else {
        return;
    };
    let bootstrap = "session-test-bootstrap-token-with-entropy";
    let secret = BootstrapSecret::from_token(bootstrap).expect("bootstrap secret");
    create_first_owner(
        &pool,
        &secret,
        bootstrap,
        FirstOwnerInput {
            email: "owner@example.test".to_owned(),
            display_name: "Session Owner".to_owned(),
            password: "correct horse battery staple".to_owned(),
        },
    )
    .await
    .expect("create owner");

    let denied = login(
        &pool,
        login_input("wrong password"),
        SessionPolicy::default(),
    )
    .await;
    assert!(matches!(denied, Err(SessionError::InvalidCredentials)));
    let logged_in = login(
        &pool,
        login_input("correct horse battery staple"),
        SessionPolicy::default(),
    )
    .await
    .expect("login owner");
    assert_eq!(logged_in.session.role, "owner");
    assert_ne!(logged_in.session_token, logged_in.csrf_token);

    let stored_token: String =
        sqlx::query_scalar("SELECT token_digest FROM volund.sessions WHERE public_id::text = $1")
            .bind(&logged_in.session.id)
            .fetch_one(&*pool)
            .await
            .expect("load stored session token");
    assert_ne!(stored_token, logged_in.session_token);

    let actor = authenticate(&pool, &logged_in.session_token, SessionPolicy::default())
        .await
        .expect("authenticate session");
    assert!(actor.csrf_matches(&logged_in.csrf_token));
    assert!(!actor.csrf_matches("wrong-csrf-token"));
    revoke_current(&pool, &actor).await.expect("revoke session");
    assert!(matches!(
        authenticate(&pool, &logged_in.session_token, SessionPolicy::default()).await,
        Err(SessionError::InvalidSession)
    ));
}

fn login_input(password: &str) -> LoginInput {
    LoginInput {
        email: " OWNER@example.test ".to_owned(),
        password: password.to_owned(),
        client_address: Some("127.0.0.1".parse().expect("loopback address")),
        user_agent: Some("VÖLUND integration test".to_owned()),
    }
}

#[tokio::test]
async fn browser_session_api_sets_verifies_and_clears_cookies() {
    let Some(pool) = test_database().await else {
        return;
    };
    create_owner(&pool).await;
    let router = api::router_with_roots(
        (*pool).clone(),
        "/tmp/volund-session-derived".into(),
        "/tmp/volund-session-web".into(),
    );
    let response = router
        .clone()
        .oneshot(json_request("POST", "/api/v1/sessions", &login_json()))
        .await
        .expect("login response");
    assert_eq!(response.status(), StatusCode::OK);
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("cookie text").to_owned())
        .collect();
    assert_eq!(cookies.len(), 2);
    assert!(cookies.iter().any(|cookie| cookie.contains("HttpOnly")));
    let session_token = cookie_value(&cookies, "volund_session");
    let csrf_token = cookie_value(&cookies, "volund_csrf");
    let cookie_header = format!("volund_session={session_token}; volund_csrf={csrf_token}");

    let current = router
        .clone()
        .oneshot(cookie_request(
            "GET",
            "/api/v1/session",
            &cookie_header,
            None,
        ))
        .await
        .expect("current session response");
    assert_eq!(current.status(), StatusCode::OK);
    assert_eq!(response_json(current).await["role"], "owner");

    verify_other_session_revocation(&router, &cookie_header, &csrf_token).await;

    let denied = router
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            "/api/v1/session",
            &cookie_header,
            None,
        ))
        .await
        .expect("CSRF denial response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let logout = router
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            "/api/v1/session",
            &cookie_header,
            Some(&csrf_token),
        ))
        .await
        .expect("logout response");
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(
        logout
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .all(|value| {
                value
                    .to_str()
                    .expect("cleared cookie")
                    .contains("Max-Age=0")
            })
    );

    let revoked = router
        .oneshot(cookie_request(
            "GET",
            "/api/v1/session",
            &cookie_header,
            None,
        ))
        .await
        .expect("revoked session response");
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
}

async fn verify_other_session_revocation(router: &axum::Router, cookie: &str, csrf: &str) {
    let second_login = router
        .clone()
        .oneshot(json_request("POST", "/api/v1/sessions", &login_json()))
        .await
        .expect("second login response");
    assert_eq!(second_login.status(), StatusCode::OK);
    let second_cookies: Vec<String> = second_login
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("cookie text").to_owned())
        .collect();
    let second_session = cookie_value(&second_cookies, "volund_session");
    let second_csrf = cookie_value(&second_cookies, "volund_csrf");
    let second_cookie = format!("volund_session={second_session}; volund_csrf={second_csrf}");
    let sessions = router
        .clone()
        .oneshot(cookie_request("GET", "/api/v1/sessions", cookie, None))
        .await
        .expect("session inventory response");
    assert_eq!(sessions.status(), StatusCode::OK);
    let sessions = response_json(sessions).await;
    assert_eq!(sessions.as_array().expect("session inventory").len(), 2);
    let other_id = sessions
        .as_array()
        .expect("session inventory")
        .iter()
        .find(|session| session["current"] == false)
        .and_then(|session| session["id"].as_str())
        .expect("other session ID");
    let revoked = router
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/sessions/{other_id}"),
            cookie,
            Some(csrf),
        ))
        .await
        .expect("other-session revocation response");
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    let denied = router
        .clone()
        .oneshot(cookie_request(
            "GET",
            "/api/v1/session",
            &second_cookie,
            None,
        ))
        .await
        .expect("revoked other-session response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn catalog_routes_fail_closed_and_viewers_cannot_mutate() {
    let Some(pool) = test_database().await else {
        return;
    };
    create_owner(&pool).await;
    let router = api::router_with_roots(
        (*pool).clone(),
        "/tmp/volund-auth-derived".into(),
        "/tmp/volund-auth-web".into(),
    );
    let anonymous = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/models")
                .body(Body::empty())
                .expect("anonymous request"),
        )
        .await
        .expect("anonymous response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let login = router
        .clone()
        .oneshot(json_request("POST", "/api/v1/sessions", &login_json()))
        .await
        .expect("viewer login response");
    let cookies: Vec<String> = login
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("cookie text").to_owned())
        .collect();
    let session_token = cookie_value(&cookies, "volund_session");
    let csrf_token = cookie_value(&cookies, "volund_csrf");
    let cookie_header = format!("volund_session={session_token}; volund_csrf={csrf_token}");
    sqlx::query("UPDATE volund.users SET role = 'viewer' WHERE email = $1")
        .bind("owner@example.test")
        .execute(&*pool)
        .await
        .expect("demote test account to viewer");

    let catalog = router
        .clone()
        .oneshot(cookie_request(
            "GET",
            "/api/v1/models",
            &cookie_header,
            None,
        ))
        .await
        .expect("viewer catalog response");
    assert_eq!(catalog.status(), StatusCode::OK);
    let libraries = router
        .clone()
        .oneshot(cookie_request(
            "GET",
            "/api/v1/libraries",
            &cookie_header,
            None,
        ))
        .await
        .expect("viewer library administration response");
    assert_eq!(libraries.status(), StatusCode::FORBIDDEN);
    let mutation = router
        .oneshot(cookie_json_request(
            "/api/v1/collections",
            &cookie_header,
            &csrf_token,
            &json!({"name": "Forbidden collection", "description": ""}),
        ))
        .await
        .expect("viewer mutation response");
    assert_eq!(mutation.status(), StatusCode::FORBIDDEN);
    assert_eq!(response_json(mutation).await["error"]["code"], "forbidden");
}

#[tokio::test]
async fn role_matrix_enforces_each_administration_boundary() {
    let Some(pool) = test_database().await else {
        return;
    };
    create_owner(&pool).await;
    let router = api::router_with_roots(
        (*pool).clone(),
        "/tmp/volund-role-derived".into(),
        "/tmp/volund-role-web".into(),
    );
    let (cookie, csrf) = login_cookie(&router).await;

    set_test_role(&pool, "viewer").await;
    assert_get_status(&router, &cookie, "/api/v1/settings", StatusCode::OK).await;
    assert_get_status(&router, &cookie, "/api/v1/users", StatusCode::FORBIDDEN).await;
    assert_get_status(&router, &cookie, "/api/v1/jobs", StatusCode::FORBIDDEN).await;
    assert_get_status(
        &router,
        &cookie,
        "/api/v1/operations/health",
        StatusCode::FORBIDDEN,
    )
    .await;
    set_test_role(&pool, "editor").await;
    assert_status(
        &router,
        cookie_json_method_request(
            "POST",
            "/api/v1/collections",
            &cookie,
            Some(&csrf),
            &json!({"name": "Editor collection", "description": ""}),
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_get_status(&router, &cookie, "/api/v1/jobs", StatusCode::FORBIDDEN).await;
    assert_get_status(&router, &cookie, "/api/v1/libraries", StatusCode::FORBIDDEN).await;
    assert_get_status(
        &router,
        &cookie,
        "/api/v1/operations/health",
        StatusCode::FORBIDDEN,
    )
    .await;

    set_test_role(&pool, "administrator").await;
    assert_get_status(&router, &cookie, "/api/v1/users", StatusCode::OK).await;
    assert_get_status(&router, &cookie, "/api/v1/jobs", StatusCode::OK).await;
    assert_get_status(&router, &cookie, "/api/v1/libraries", StatusCode::OK).await;
    assert_get_status(
        &router,
        &cookie,
        "/api/v1/operations/health",
        StatusCode::OK,
    )
    .await;
    assert_status(
        &router,
        cookie_json_method_request(
            "PUT",
            "/api/v1/settings/instance.name",
            &cookie,
            Some(&csrf),
            &json!({"value": "Authorization Matrix", "expectedRevision": 0}),
        ),
        StatusCode::OK,
    )
    .await;

    set_test_role(&pool, "owner").await;
    assert_get_status(&router, &cookie, "/api/v1/users", StatusCode::OK).await;
}

#[tokio::test]
async fn every_unsafe_http_verb_requires_csrf_and_same_origin() {
    let Some(pool) = test_database().await else {
        return;
    };
    create_owner(&pool).await;
    let router = api::router_with_roots(
        (*pool).clone(),
        "/tmp/volund-csrf-derived".into(),
        "/tmp/volund-csrf-web".into(),
    );
    let (cookie, csrf) = login_cookie(&router).await;
    for (method, uri, body) in [
        (
            "POST",
            "/api/v1/collections",
            json!({"name": "Denied", "description": ""}),
        ),
        (
            "PUT",
            "/api/v1/settings/instance.name",
            json!({"value": "Denied", "expectedRevision": 0}),
        ),
        (
            "PATCH",
            "/api/v1/users/00000000-0000-0000-0000-000000000000",
            json!({"status": "active"}),
        ),
        ("DELETE", "/api/v1/session", json!({})),
    ] {
        assert_status(
            &router,
            cookie_json_method_request(method, uri, &cookie, None, &body),
            StatusCode::FORBIDDEN,
        )
        .await;
    }
    let cross_origin = Request::builder()
        .method("POST")
        .uri("/api/v1/collections")
        .header(header::HOST, "vault.example")
        .header(header::ORIGIN, "http://attacker.example")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-csrf-token", csrf)
        .body(Body::from(
            json!({"name": "Denied", "description": ""}).to_string(),
        ))
        .expect("cross-origin request");
    assert_status(&router, cross_origin, StatusCode::FORBIDDEN).await;
}

async fn create_owner(pool: &sqlx::PgPool) {
    let bootstrap = "browser-api-bootstrap-token-with-entropy";
    let secret = BootstrapSecret::from_token(bootstrap).expect("bootstrap secret");
    create_first_owner(
        pool,
        &secret,
        bootstrap,
        FirstOwnerInput {
            email: "owner@example.test".to_owned(),
            display_name: "Browser Owner".to_owned(),
            password: "correct horse battery staple".to_owned(),
        },
    )
    .await
    .expect("create browser owner");
}

fn login_json() -> Value {
    json!({"email": "owner@example.test", "password": "correct horse battery staple"})
}

fn json_request(method: &str, uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build JSON request")
}

fn cookie_request(method: &str, uri: &str, cookie: &str, csrf: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    builder.body(Body::empty()).expect("build cookie request")
}

fn cookie_json_request(uri: &str, cookie: &str, csrf: &str, body: &Value) -> Request<Body> {
    cookie_json_method_request("POST", uri, cookie, Some(csrf), body)
}

fn cookie_json_method_request(
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: &Value,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("build authenticated JSON request")
}

async fn login_cookie(router: &axum::Router) -> (String, String) {
    let response = router
        .clone()
        .oneshot(json_request("POST", "/api/v1/sessions", &login_json()))
        .await
        .expect("login response");
    assert_eq!(response.status(), StatusCode::OK);
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("cookie text").to_owned())
        .collect();
    let session = cookie_value(&cookies, "volund_session");
    let csrf = cookie_value(&cookies, "volund_csrf");
    (
        format!("volund_session={session}; volund_csrf={csrf}"),
        csrf,
    )
}

async fn set_test_role(pool: &sqlx::PgPool, role: &str) {
    sqlx::query("UPDATE volund.users SET role = $1 WHERE email = 'owner@example.test'")
        .bind(role)
        .execute(pool)
        .await
        .expect("set matrix role");
}

async fn assert_status(router: &axum::Router, request: Request<Body>, expected: StatusCode) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("route response");
    assert_eq!(response.status(), expected);
}

async fn assert_get_status(router: &axum::Router, cookie: &str, path: &str, expected: StatusCode) {
    assert_status(router, cookie_request("GET", path, cookie, None), expected).await;
}

fn cookie_value(cookies: &[String], name: &str) -> String {
    cookies
        .iter()
        .find_map(|cookie| {
            cookie
                .strip_prefix(&format!("{name}="))
                .and_then(|value| value.split(';').next())
        })
        .expect("named cookie")
        .to_owned()
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&body).expect("parse JSON response")
}
