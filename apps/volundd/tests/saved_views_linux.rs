#![cfg(target_os = "linux")]
//! Saved model list views (ADR 0032): validated, per-account, bounded.
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, get_json, test_database};
use tower::ServiceExt;

async fn send(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
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
#[allow(clippy::too_many_lines)] // One account walks through every saved-view rule.
async fn saved_views_are_validated_owned_and_bounded() {
    let Some(pool) = test_database().await else {
        return;
    };
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;
    let tag = "11111111-2222-3333-4444-555555555555";

    let (status, created) = send(
        &router,
        Method::POST,
        "/api/v1/saved-views",
        Some(json!({
            "name": "  Halterungen  ",
            "query": {"q": "  bracket ", "tag": tag, "kind": "part", "sort": "updated"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["name"], "Halterungen");
    // Normalized like a live search: trimmed text, the default sort omitted.
    assert_eq!(
        created["query"],
        json!({"q": "bracket", "tag": tag, "kind": "part"})
    );

    let (status, _) = send(
        &router,
        Method::POST,
        "/api/v1/saved-views",
        Some(json!({
            "name": "HALTERUNGEN", "query": {}
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "names are unique per account, ignoring case"
    );
    for invalid in [
        json!({"name": "Bad sort", "query": {"sort": "random"}}),
        json!({"name": "Bad kind", "query": {"kind": "statue"}}),
        json!({"name": "", "query": {}}),
        json!({"name": "x".repeat(81), "query": {}}),
        json!({"name": "Paged", "query": {"offset": 48}}),
    ] {
        let (status, body) = send(
            &router,
            Method::POST,
            "/api/v1/saved-views",
            Some(invalid.clone()),
        )
        .await;
        assert!(
            status.is_client_error() && status != StatusCode::CONFLICT,
            "{invalid}: {status} {body}"
        );
    }

    // Another account's views are neither listed nor removable.
    let foreign: String = sqlx::query_scalar(
        "WITH other AS (INSERT INTO volund.users (email,normalized_email,display_name,role,status) \
         VALUES ('other@example.test','other@example.test','Other','viewer','active') RETURNING id) \
         INSERT INTO volund.saved_model_views (user_id,name,query) SELECT id,'Fremd','{}' FROM other \
         RETURNING public_id::text",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    let (status, views) = get_json(&router, "/api/v1/saved-views").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(views.as_array().unwrap().len(), 1);
    let (status, _) = send(
        &router,
        Method::DELETE,
        &format!("/api/v1/saved-views/{foreign}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let id = created["id"].as_str().unwrap();
    let (status, _) = send(
        &router,
        Method::DELETE,
        &format!("/api/v1/saved-views/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(get_json(&router, "/api/v1/saved-views").await.1, json!([]));

    for index in 0..50 {
        let (status, _) = send(
            &router,
            Method::POST,
            "/api/v1/saved-views",
            Some(json!({
                "name": format!("View {index:02}"), "query": {}
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, body) = send(
        &router,
        Method::POST,
        "/api/v1/saved-views",
        Some(json!({
            "name": "One too many", "query": {}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "saved_view_limit");
    let names: Vec<String> = get_json(&router, "/api/v1/saved-views")
        .await
        .1
        .as_array()
        .unwrap()
        .iter()
        .map(|view| view["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names.first().map(String::as_str), Some("View 00"));
    assert_eq!(names.len(), 50);
}
