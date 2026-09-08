#![cfg(target_os = "linux")]

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::api;

mod support;

use support::{authenticated_router, get_json, test_database};

async fn mutation(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build author request"),
        )
        .await
        .expect("author request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read author response");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("author JSON")
    };
    (status, value)
}

#[tokio::test]
async fn authors_are_revisioned_and_merge_models_atomically() {
    let Some(pool) = test_database().await else {
        return;
    };
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let create = |name: &str| {
        serde_json::json!({
            "name": name, "website": "https://example.test/profile",
            "provenanceSource": "website", "provenanceNote": "Published profile"
        })
    };
    let (status, source) = mutation(
        &router,
        Method::POST,
        "/api/v1/authors",
        create("Ｍaker  Lab"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let source_id = source["id"].as_str().expect("source ID");
    let (status, collision) = mutation(
        &router,
        Method::POST,
        "/api/v1/authors",
        create("maker lab"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(collision["error"]["code"], "conflict");
    let (_, target) = mutation(&router, Method::POST, "/api/v1/authors", create("Target")).await;
    let target_id = target["id"].as_str().expect("target ID");
    let source_internal: i64 =
        sqlx::query_scalar("SELECT id FROM volund.authors WHERE public_id::text=$1")
            .bind(source_id)
            .fetch_one(&*pool)
            .await
            .expect("source internal ID");
    sqlx::query(
        "INSERT INTO volund.models (slug,name,kind,author_id) VALUES ('merge-fixture','Merge fixture','part',$1)",
    )
    .bind(source_internal)
    .execute(&*pool)
    .await
    .expect("insert merge fixture");
    let (status, denied) = mutation(
        &router,
        Method::POST,
        &format!("/api/v1/authors/{source_id}/merge"),
        serde_json::json!({"targetAuthorId":target_id,"confirmation":"wrong"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(denied["error"]["code"], "bad_request");
    let confirmation = format!("MERGE AUTHOR {source_id} INTO {target_id}");
    let (status, surviving) = mutation(
        &router,
        Method::POST,
        &format!("/api/v1/authors/{source_id}/merge"),
        serde_json::json!({"targetAuthorId":target_id,"confirmation":confirmation}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(surviving["id"], target_id);
    assert_eq!(surviving["modelCount"], 1);
    let (_, merged) = get_json(&router, &format!("/api/v1/authors/{source_id}")).await;
    assert_eq!(merged["active"], false);
    assert_eq!(merged["mergedIntoId"], target_id);
    let audit: Vec<(String, String)> = sqlx::query_as(
        "SELECT action,outcome FROM volund.security_audit_events \
         WHERE target_public_id::text=$1 ORDER BY id",
    )
    .bind(source_id)
    .fetch_all(&*pool)
    .await
    .expect("author audit rows");
    assert!(audit.contains(&("author.merge".to_owned(), "denied".to_owned())));
    assert!(audit.contains(&("author.merge".to_owned(), "success".to_owned())));
}
