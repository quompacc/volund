#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::{api, scan_pipeline};

mod support;

use support::{authenticated_router, test_database};

#[tokio::test]
#[allow(clippy::too_many_lines)] // One ordered lifecycle over shared library and audit state.
async fn owner_can_validate_create_disable_enable_and_scan_a_library() {
    let Some(pool) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let library_path = std::env::temp_dir().join(format!("volund-library-admin-{suffix}"));
    fs::create_dir(&library_path).expect("create isolated library directory");
    let path = library_path.to_string_lossy().into_owned();
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;

    let (status, validation) = request(
        &router,
        "POST",
        "/api/v1/libraries/validate",
        Some(json!({"path": path})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validation["readable"], true);
    assert_eq!(validation["directory"], true);

    assert_create_confirmation_denied(&router, &path).await;
    let (status, created) = request(
        &router,
        "POST",
        "/api/v1/libraries",
        Some(json!({"key": "admin_test", "name": "Admin Test", "path": path, "confirmation": "ADD LIBRARY admin_test"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["enabled"], true);
    assert_eq!(created["filesystemPath"], validation["canonicalPath"]);
    assert_eq!(created["storage"]["state"], "healthy");
    assert_eq!(created["storage"]["reachable"], true);
    assert_eq!(created["storage"]["directory"], true);
    assert_eq!(created["storage"]["readable"], true);
    assert!(created["storage"]["availableBytes"].as_u64().is_some());

    let (status, disabled) = request(
        &router,
        "PATCH",
        "/api/v1/libraries/admin_test",
        Some(json!({"expectedRevision": 1, "name": "Admin Test Archive", "enabled": false, "confirmation": "UPDATE LIBRARY admin_test"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled["name"], "Admin Test Archive");
    assert_eq!(disabled["enabled"], false);
    let (status, _) = request(
        &router,
        "POST",
        "/api/v1/libraries/admin_test/scans",
        Some(json!({"full": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = request(
        &router,
        "PATCH",
        "/api/v1/libraries/admin_test",
        Some(json!({"expectedRevision": 2, "enabled": true, "confirmation": "UPDATE LIBRARY admin_test"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_full_scan_confirmation_denied(&router).await;
    let (status, scan) = request(
        &router,
        "POST",
        "/api/v1/libraries/admin_test/scans",
        Some(json!({"full": true, "confirmation": "FULL SCAN admin_test"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(scan["status"], "queued");
    assert_eq!(scan["full"], true);
    assert!(scan["id"].as_str().is_some());
    let durable_status: String =
        sqlx::query_scalar("SELECT status FROM volund.scan_runs WHERE public_id::text = $1")
            .bind(scan["id"].as_str().expect("scan public ID"))
            .fetch_one(&*pool)
            .await
            .expect("load durable queued scan");
    assert_eq!(durable_status, "queued");
    let processed = scan_pipeline::process_next(&pool)
        .await
        .expect("process scan queue")
        .expect("queued scan");
    assert_eq!(processed.status, "completed");
    assert_eq!(processed.scan.expect("scan report").discovered_files, 0);

    let (status, libraries) = request(&router, "GET", "/api/v1/libraries", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(libraries.as_array().expect("library list").len(), 1);
    assert_eq!(libraries[0]["latestScanStatus"], "completed");
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action IN ('library.create', 'library.update', 'library.scan.queued')",
    )
    .fetch_one(&*pool)
    .await
    .expect("count library audit events");
    assert_eq!(audits, 5);
    let denied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action IN ('library.create', 'library.update', 'library.scan.queued') \
         AND outcome='denied'",
    )
    .fetch_one(&*pool)
    .await
    .expect("count denied library audits");
    assert_eq!(denied, 1);

    assert_missing_library_is_blocked(&router, &library_path).await;
}

async fn assert_create_confirmation_denied(router: &axum::Router, path: &str) {
    let (status, _) = request(
        router,
        "POST",
        "/api/v1/libraries",
        Some(json!({"key": "admin_test", "name": "Admin Test", "path": path, "confirmation": "wrong"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn assert_full_scan_confirmation_denied(router: &axum::Router) {
    let (status, _) = request(
        router,
        "POST",
        "/api/v1/libraries/admin_test/scans",
        Some(json!({"full": true, "confirmation": "wrong"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn assert_missing_library_is_blocked(router: &axum::Router, library_path: &std::path::Path) {
    fs::remove_dir(library_path).expect("remove isolated empty library directory");
    let (status, libraries) = request(router, "GET", "/api/v1/libraries", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(libraries[0]["storage"]["state"], "blocked");
    assert_eq!(libraries[0]["storage"]["reachable"], false);
    let (status, error) = request(
        router,
        "POST",
        "/api/v1/libraries/admin_test/scans",
        Some(json!({"full": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "conflict");
}

async fn request(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = if let Some(value) = body {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .expect("build library request")
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("build library request")
    };
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("library response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read library response");
    (
        status,
        serde_json::from_slice(&body).expect("JSON library response"),
    )
}
