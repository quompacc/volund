#![cfg(target_os = "linux")]

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;

use support::{authenticated_router, get_json, test_database};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

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
                .expect("build model validation request"),
        )
        .await
        .expect("model validation response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read validation response");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("validation JSON")
    };
    (status, value)
}

fn update(file_id: &str) -> Value {
    serde_json::json!({
        "expectedRevision": 1,
        "name": "Unicode Ä Modell",
        "description": "Beschreibung",
        "kind": "part",
        "licenseKind": "not-specified",
        "licenseValue": null,
        "authorName": null,
        "tags": [],
        "tagIds": [],
        "collectionIds": [],
        "primaryFileId": file_id,
        "viewerRotation": [0, 0, 0]
    })
}

fn invalid_creates(step_id: &str, pdf_id: &str) -> Vec<(Value, StatusCode, &'static str)> {
    vec![
        (
            serde_json::json!({"name":" ","slug":"empty-name","kind":"part","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model name",
        ),
        (
            serde_json::json!({"name":"ü".repeat(161),"slug":"long-name","kind":"part","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model name",
        ),
        (
            serde_json::json!({"name":"Bad slug","slug":"Bad--Slug","kind":"part","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model slug",
        ),
        (
            serde_json::json!({"name":"Unicode slug","slug":"mödell","kind":"part","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model slug",
        ),
        (
            serde_json::json!({"name":"Long slug","slug":"a".repeat(161),"kind":"part","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model slug",
        ),
        (
            serde_json::json!({"name":"Bad kind","slug":"bad-kind","kind":"folder","primaryFileId":step_id}),
            StatusCode::BAD_REQUEST,
            "model kind",
        ),
        (
            serde_json::json!({"name":"Unknown source","slug":"unknown-source","kind":"part","primaryFileId":"not-an-id"}),
            StatusCode::NOT_FOUND,
            "unknown primary",
        ),
        (
            serde_json::json!({"name":"PDF source","slug":"pdf-source","kind":"part","primaryFileId":pdf_id}),
            StatusCode::BAD_REQUEST,
            "CAD or mesh",
        ),
    ]
}

fn invalid_updates(step_id: &str, pdf_id: &str) -> Vec<(Value, StatusCode, &'static str)> {
    let mut cases = Vec::new();
    let mut body = update(step_id);
    body["name"] = Value::String(" ".to_owned());
    cases.push((body, StatusCode::BAD_REQUEST, "model name"));
    let mut body = update(step_id);
    body["description"] = Value::String("ü".repeat(4_001));
    cases.push((body, StatusCode::BAD_REQUEST, "description"));
    let mut body = update(step_id);
    body["authorName"] = Value::String("Ä".repeat(161));
    cases.push((body, StatusCode::BAD_REQUEST, "metadata name"));
    let mut body = update(step_id);
    body["licenseKind"] = Value::String("spdx".to_owned());
    body["licenseValue"] = Value::String("invented-license".to_owned());
    cases.push((body, StatusCode::BAD_REQUEST, "license"));
    let mut body = update(step_id);
    body["viewerRotation"] = serde_json::json!([361, 0, 0]);
    cases.push((body, StatusCode::BAD_REQUEST, "viewer rotation"));
    cases.push((
        update(pdf_id),
        StatusCode::BAD_REQUEST,
        "belong to the model",
    ));
    let mut body = update(step_id);
    body["tagIds"] = serde_json::json!(["00000000-0000-0000-0000-000000000001"]);
    cases.push((body, StatusCode::NOT_FOUND, "tag not found"));
    let mut body = update(step_id);
    body["collectionIds"] = serde_json::json!(["00000000-0000-0000-0000-000000000001"]);
    cases.push((body, StatusCode::NOT_FOUND, "collection not found"));
    cases
}

#[tokio::test]
async fn model_inputs_and_references_fail_with_stable_errors_and_no_partial_change() {
    let Some(pool) = test_database().await else {
        return;
    };
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let root_key = format!("model_validation_{unique}");
    let root = std::env::temp_dir().join(format!("volund-model-validation-{unique}"));
    fs::create_dir_all(&root).expect("create model validation root");
    fs::write(
        root.join("valid.step"),
        b"ISO-10303-21;ENDSEC;END-ISO-10303-21;",
    )
    .expect("write STEP fixture");
    fs::write(root.join("manual.pdf"), b"%PDF-1.7 model validation").expect("write PDF fixture");
    scanner::register_root(&pool, &root_key, "Model validation", &root)
        .await
        .expect("register validation root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan validation root");
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let (_, files) = get_json(&router, &format!("/api/v1/roots/{root_key}/files?limit=20")).await;
    let items = files["items"].as_array().expect("source items");
    let source_id = |path: &str| {
        items
            .iter()
            .find(|item| item["path"] == path)
            .and_then(|item| item["id"].as_str())
            .expect("source ID")
    };
    let step_id = source_id("valid.step");
    let pdf_id = source_id("manual.pdf");

    for (body, expected_status, expected_message) in invalid_creates(step_id, pdf_id) {
        let (status, error) = mutation(&router, Method::POST, "/api/v1/models", body).await;
        assert_eq!(status, expected_status);
        assert_eq!(
            error["error"]["code"],
            expected_status_code(expected_status)
        );
        assert!(
            error["error"]["message"]
                .as_str()
                .is_some_and(|value| value.contains(expected_message))
        );
    }

    let (status, model) = mutation(
        &router,
        Method::POST,
        "/api/v1/models",
        serde_json::json!({"name":"Unicode Ä Modell","slug":"unicode-model","kind":"part","primaryFileId":step_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let model_id = model["id"].as_str().expect("model ID");
    let (status, duplicate) = mutation(
        &router,
        Method::POST,
        "/api/v1/models",
        serde_json::json!({"name":"Anderer Anzeigename","slug":"unicode-model","kind":"part","primaryFileId":step_id}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(duplicate["error"]["code"], "conflict");

    let (status, unknown) = get_json(&router, "/api/v1/models/not-an-id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(unknown["error"]["code"], "not_found");

    for (body, expected_status, expected_message) in invalid_updates(step_id, pdf_id) {
        let (status, error) = mutation(
            &router,
            Method::PATCH,
            &format!("/api/v1/models/{model_id}"),
            body,
        )
        .await;
        assert_eq!(status, expected_status);
        assert!(
            error["error"]["message"]
                .as_str()
                .is_some_and(|value| value.contains(expected_message))
        );
    }

    let (_, unchanged) = get_json(&router, &format!("/api/v1/models/{model_id}")).await;
    assert_eq!(unchanged["revision"], 1);
    assert_eq!(unchanged["name"], "Unicode Ä Modell");
    let success_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE target_type='model' AND outcome='success'",
    )
    .fetch_one(&*pool)
    .await
    .expect("count committed model audits");
    assert_eq!(success_audits, 1);
    fs::remove_dir_all(root).expect("remove model validation root");
}

fn expected_status_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::NOT_FOUND => "not_found",
        _ => panic!("unexpected validation status"),
    }
}
