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
                .expect("build metadata request"),
        )
        .await
        .expect("metadata request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read metadata response");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("metadata JSON")
    };
    (status, value)
}

#[tokio::test]
async fn collections_are_revisioned_and_removal_preserves_models() {
    let Some(pool) = test_database().await else {
        return;
    };
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let (status, collection) = mutation(
        &router,
        Method::POST,
        "/api/v1/collections",
        serde_json::json!({"name":"Ｐrint  Jobs","description":"Fixture"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = collection["id"].as_str().expect("collection ID");
    let (status, collision) = mutation(
        &router,
        Method::POST,
        "/api/v1/collections",
        serde_json::json!({"name":"print jobs","description":"Duplicate"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(collision["error"]["code"], "conflict");
    let model_id: String = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind) VALUES ('collection-fixture','Collection fixture','part') RETURNING public_id::text")
        .fetch_one(&*pool).await.expect("insert model");
    let uri = format!("/api/v1/collections/{id}/models/{model_id}");
    let body = serde_json::json!({"expectedRevision":1});
    let (left, right) = tokio::join!(
        mutation(&router, Method::PUT, &uri, body.clone()),
        mutation(&router, Method::PUT, &uri, body),
    );
    let statuses = [left.0, right.0];
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    let (_, detail) = get_json(&router, &format!("/api/v1/collections/{id}")).await;
    assert_eq!(detail["modelIds"], serde_json::json!([model_id]));
    assert_eq!(detail["revision"], 2);
    let confirmation = format!("REMOVE COLLECTION {id}");
    let (status, removed) = mutation(
        &router,
        Method::DELETE,
        &format!("/api/v1/collections/{id}"),
        serde_json::json!({"expectedRevision":2,"confirmation":confirmation}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["active"], false);
    let model_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM volund.models WHERE public_id::text=$1)")
            .bind(&model_id)
            .fetch_one(&*pool)
            .await
            .expect("check preserved model");
    assert!(model_exists);
}

#[tokio::test]
async fn tag_merge_deduplicates_links_and_retains_an_alias() {
    let Some(pool) = test_database().await else {
        return;
    };
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let (_, source) = mutation(
        &router,
        Method::POST,
        "/api/v1/tags",
        serde_json::json!({"name":"Core XY"}),
    )
    .await;
    let source_id = source["id"].as_str().expect("source tag ID");
    let (status, collision) = mutation(
        &router,
        Method::POST,
        "/api/v1/tags",
        serde_json::json!({"name":"Ｃore  xy"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(collision["error"]["code"], "conflict");
    let (_, target) = mutation(
        &router,
        Method::POST,
        "/api/v1/tags",
        serde_json::json!({"name":"Printer"}),
    )
    .await;
    let target_id = target["id"].as_str().expect("target tag ID");
    let model_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind) VALUES ('tag-fixture','Tag fixture','part') RETURNING id")
        .fetch_one(&*pool).await.expect("insert model");
    sqlx::query("INSERT INTO volund.model_tags (model_id,tag_id) SELECT $1,id FROM volund.tags WHERE public_id::text IN ($2,$3)")
        .bind(model_id).bind(source_id).bind(target_id).execute(&*pool).await.expect("attach both tags");
    let denied = mutation(
        &router,
        Method::POST,
        &format!("/api/v1/tags/{source_id}/merge"),
        serde_json::json!({"expectedRevision":1,"targetTagId":target_id,"confirmation":"wrong"}),
    )
    .await;
    assert_eq!(denied.0, StatusCode::BAD_REQUEST);
    let confirmation = format!("MERGE TAG {source_id} INTO {target_id}");
    let (status, surviving) = mutation(&router, Method::POST, &format!("/api/v1/tags/{source_id}/merge"),
        serde_json::json!({"expectedRevision":1,"targetTagId":target_id,"confirmation":confirmation})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(surviving["id"], target_id);
    let (_, alias) = get_json(&router, &format!("/api/v1/tags/{source_id}")).await;
    assert_eq!(alias["active"], false);
    assert_eq!(alias["mergedIntoId"], target_id);
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.model_tags WHERE model_id=$1")
        .bind(model_id)
        .fetch_one(&*pool)
        .await
        .expect("count deduplicated tags");
    assert_eq!(links, 1);
    let remove_confirmation = format!("REMOVE TAG {target_id}");
    let (status, removed) = mutation(
        &router,
        Method::DELETE,
        &format!("/api/v1/tags/{target_id}"),
        serde_json::json!({"expectedRevision":1,"confirmation":remove_confirmation}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["active"], false);
    let model_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM volund.models WHERE id=$1)")
            .bind(model_id)
            .fetch_one(&*pool)
            .await
            .expect("check model");
    assert!(model_exists);
    let audit: Vec<(String, String)> = sqlx::query_as(
        "SELECT action,outcome FROM volund.security_audit_events WHERE target_public_id::text=$1 ORDER BY id")
        .bind(source_id).fetch_all(&*pool).await.expect("tag audit");
    assert!(audit.contains(&("tag.merge".to_owned(), "denied".to_owned())));
    assert!(audit.contains(&("tag.merge".to_owned(), "success".to_owned())));
}
