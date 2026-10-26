#![cfg(target_os = "linux")]
//! Bulk model edit (ADR 0034): all-or-nothing, revision-checked, audited.
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, test_database};
use tower::ServiceExt;

async fn post(router: &axum::Router, body: &Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/models/bulk-edit")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
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

async fn models(pool: &sqlx::PgPool) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT public_id::text, revision FROM volund.models ORDER BY slug")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One catalog walks through every bulk-edit rule.
async fn bulk_edit_applies_everything_or_nothing() {
    let Some(pool) = test_database().await else {
        return;
    };
    sqlx::query(
        "INSERT INTO volund.models (slug,name,kind) SELECT 'model-'||lpad(n::text,2,'0'),'Model '||n,'part' \
         FROM generate_series(1,12) n",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let tags: Vec<String> = sqlx::query_scalar(
        "INSERT INTO volund.tags (slug,name,normalized_name) VALUES ('keep','Keep','keep'),('drop','Drop','drop') \
         RETURNING public_id::text",
    )
    .fetch_all(&*pool)
    .await
    .unwrap();
    let (keep, drop) = (&tags[0], &tags[1]);
    sqlx::query("INSERT INTO volund.model_tags (model_id,tag_id) SELECT model.id,tag.id FROM volund.models model, volund.tags tag WHERE tag.slug='drop'")
        .execute(&*pool)
        .await
        .unwrap();
    let collection: String = sqlx::query_scalar(
        "INSERT INTO volund.collections (slug,name,normalized_name) VALUES ('shelf','Shelf','shelf') RETURNING public_id::text",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;
    let all = models(&pool).await;
    let three: Vec<Value> = all[..3]
        .iter()
        .map(|(id, revision)| json!({"id": id, "expectedRevision": revision}))
        .collect();

    let (status, updated) = post(&router, &json!({
        "models": three,
        "changes": {"addTagIds": [keep], "removeTagIds": [drop], "addCollectionIds": [collection],
            "kind": "assembly", "authorName": "Grace Hopper", "licenseKind": "spdx", "licenseValue": "MIT"}
    })).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated.as_array().unwrap().len(), 3);
    for model in updated.as_array().unwrap() {
        assert_eq!(model["kind"], "assembly");
        assert_eq!(model["authorName"], "Grace Hopper");
        assert_eq!(
            (
                model["licenseKind"].as_str(),
                model["licenseValue"].as_str()
            ),
            (Some("spdx"), Some("MIT"))
        );
        assert_eq!(model["tags"], json!(["Keep"]));
        assert_eq!(model["collections"], json!(["Shelf"]));
        assert_eq!(model["revision"], 2);
    }
    let untouched: (String, i64) = sqlx::query_as(
        "SELECT kind, (SELECT count(*) FROM volund.model_tags WHERE model_id=model.id) FROM volund.models model WHERE slug='model-04'",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert_eq!(
        untouched,
        ("part".to_owned(), 1),
        "unselected models stay unchanged"
    );
    let ordinals: Vec<i32> =
        sqlx::query_scalar("SELECT ordinal FROM volund.collection_models ORDER BY ordinal")
            .fetch_all(&*pool)
            .await
            .unwrap();
    assert_eq!(ordinals, [0, 1, 2]);
    let (events, batches): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT metadata->>'bulkBatch') FROM volund.security_audit_events \
         WHERE action='model.update' AND metadata ? 'bulkBatch'",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert_eq!(
        (events, batches),
        (3, 1),
        "one audit event per model, one shared batch"
    );

    // A stale revision on one model rejects the whole batch and names it.
    let (status, body) = post(&router, &json!({
        "models": [{"id": all[0].0, "expectedRevision": 2}, {"id": all[1].0, "expectedRevision": 1}],
        "changes": {"kind": "project", "authorName": ""}
    })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&all[1].0)
    );
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM volund.models WHERE slug IN ('model-01','model-02') ORDER BY slug",
    )
    .fetch_all(&*pool)
    .await
    .unwrap();
    assert_eq!(
        kinds,
        ["assembly", "assembly"],
        "nothing changed in a rejected batch"
    );

    // An empty author name clears the author.
    let (status, cleared) = post(
        &router,
        &json!({
            "models": [{"id": all[0].0, "expectedRevision": 2}], "changes": {"authorName": ""}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared[0]["authorName"], Value::Null);
    assert_eq!(
        cleared[0]["kind"], "assembly",
        "fields left out stay unchanged"
    );

    // Ten or more models need the exact count as confirmation.
    let current = models(&pool).await;
    let ten: Vec<Value> = current[2..12]
        .iter()
        .map(|(id, revision)| json!({"id": id, "expectedRevision": revision}))
        .collect();
    let request = json!({"models": ten, "changes": {"addTagIds": [keep]}});
    assert_eq!(post(&router, &request).await.0, StatusCode::BAD_REQUEST);
    let mut confirmed = request.clone();
    confirmed["confirmation"] = json!("EDIT 10 MODELS");
    assert_eq!(post(&router, &confirmed).await.0, StatusCode::OK);

    let one = |changes: Value| json!({"models": [{"id": current[0].0, "expectedRevision": current[0].1}], "changes": changes});
    for (changes, expected) in [
        (json!({}), StatusCode::BAD_REQUEST),
        (json!({"kind": "statue"}), StatusCode::BAD_REQUEST),
        (
            json!({"licenseKind": "spdx", "licenseValue": "Not-A-License"}),
            StatusCode::BAD_REQUEST,
        ),
        (json!({"licenseValue": "MIT"}), StatusCode::BAD_REQUEST),
        (
            json!({"addTagIds": [keep], "removeTagIds": [keep]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"addTagIds": ["11111111-2222-3333-4444-555555555555"]}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            post(&router, &one(changes.clone())).await.0,
            expected,
            "{changes}"
        );
    }
    let duplicate = json!({"models": [{"id": current[0].0, "expectedRevision": 1}, {"id": current[0].0, "expectedRevision": 1}],
        "changes": {"kind": "part"}});
    assert_eq!(post(&router, &duplicate).await.0, StatusCode::BAD_REQUEST);
}
