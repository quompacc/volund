#![cfg(target_os = "linux")]
//! Stralar proxy (ADR 0042): `/stralar/*` reaches the Stralar service only for signed-in
//! browser sessions, carries the account identity and the shared secret, never VÖLUND's
//! credentials, and refuses cross-origin mutations and bearer tokens.
mod support;

use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::any,
};
use serde_json::{Value, json};
use support::{authenticated_router, owner_session, test_database};
use tower::ServiceExt;
use volundd::stralar_proxy::{StralarConfig, router_with_config};

const SECRET: &str = "stralar-integration-test-secret-0123456789";

/// A stand-in for Stralar that reports what it received.
async fn start_echo_upstream() -> String {
    async fn echo(
        method: Method,
        headers: HeaderMap,
        uri: axum::http::Uri,
        body: Bytes,
    ) -> Json<Value> {
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        Json(json!({
            "method": method.as_str(),
            "path": uri.to_string(),
            "user": text("x-stralar-user"),
            "secret": text("x-stralar-proxy-secret"),
            "cookie": text("cookie"),
            "csrf": text("x-csrf-token"),
            "host": text("host"),
            "bodyBytes": body.len(),
        }))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(any(echo)))
            .await
            .unwrap();
    });
    format!("http://127.0.0.1:{}", address.port())
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(header::HOST, "volund.test")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One session walks through every boundary of the proxy.
async fn stralar_proxy_forwards_identity_and_guards_the_boundary() {
    let Some(pool) = test_database().await else {
        return;
    };
    let upstream = start_echo_upstream().await;
    let config = StralarConfig::from_values(&upstream, SECRET);
    assert!(matches!(config, StralarConfig::Enabled { .. }));
    let browser =
        authenticated_router(&pool, router_with_config((*pool).clone(), config.clone())).await;
    let anonymous = router_with_config((*pool).clone(), config);
    let user_id: String = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.users WHERE email = 'catalog-owner@example.test'",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Signed-in GET: prefix stripped, identity + secret added, VÖLUND credentials removed.
    let (status, _, seen) = send(&browser, get("/stralar/api/model?id=7")).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["path"], "/api/model?id=7");
    assert_eq!(seen["user"], user_id.as_str());
    assert_eq!(seen["secret"], SECRET);
    assert_eq!(
        seen["cookie"],
        Value::Null,
        "the VÖLUND session cookie must not reach Stralar"
    );
    assert_eq!(seen["csrf"], Value::Null);
    assert!(seen["host"].as_str().unwrap().starts_with("127.0.0.1:"));

    // A client cannot choose its identity.
    let forged = Request::builder()
        .uri("/stralar/api/model")
        .header(header::HOST, "volund.test")
        .header("x-stralar-user", "00000000-0000-4000-8000-000000000000")
        .header("x-stralar-proxy-secret", "guess")
        .body(Body::empty())
        .unwrap();
    let (_, _, seen) = send(&browser, forged).await;
    assert_eq!(seen["user"], user_id.as_str());
    assert_eq!(seen["secret"], SECRET);

    // Mutations need a present same-origin Origin (Stralar sends no CSRF token).
    let post = |origin: Option<&str>| {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/stralar/api/slice")
            .header(header::HOST, "volund.test")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        builder.body(Body::from("{\"x\":1}")).unwrap()
    };
    let (status, _, _) = send(&browser, post(None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = send(&browser, post(Some("http://evil.test"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, seen) = send(&browser, post(Some("http://volund.test"))).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["method"], "POST");
    assert_eq!(seen["bodyBytes"], 7);

    // No session → 401; bearer tokens never reach Stralar → 403.
    let (status, _, _) = send(&anonymous, get("/stralar/api/model")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let bearer = Request::builder()
        .uri("/stralar/api/model")
        .header(header::HOST, "volund.test")
        .header(header::AUTHORIZATION, "Bearer vlnd_0000")
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&anonymous, bearer).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // `/stralar` → `/stralar/`.
    let (status, headers, _) = send(&browser, get("/stralar")).await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "/stralar/");

    // Disabled integration: the route does not exist; misconfigured: 503.
    let disabled = router_with_config((*pool).clone(), StralarConfig::Disabled);
    let (status, _, _) = send(&disabled, get("/stralar/api/model")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let broken = owner_session(
        &pool,
        router_with_config(
            (*pool).clone(),
            StralarConfig::from_values("http://10.0.0.1:1", SECRET),
        ),
    )
    .await;
    let (status, _, body) = send(&broken, get("/stralar/api/model")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "stralar_misconfigured");
}
