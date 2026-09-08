#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;

use support::{authenticated_router, test_database};

#[tokio::test]
async fn project_media_streams_and_preview_requests_are_durable() {
    let Some(pool) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-media-{suffix}"));
    fs::create_dir_all(root.join("Documents")).expect("create media fixture");
    fs::write(root.join("part.step"), b"0123456789").expect("write CAD fixture");
    fs::write(root.join("Documents/manual.pdf"), b"pdf fixture").expect("write document");
    scanner::register_root(&pool, "media_test", "Media Test", &root)
        .await
        .expect("register media root");
    let report = scanner::scan_root(&pool, "media_test", false)
        .await
        .expect("scan all project media");
    assert_eq!(report.discovered_files, 2);
    let file_id: String = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.source_files WHERE relative_path = 'part.step'",
    )
    .fetch_one(&*pool)
    .await
    .expect("load source identity");
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/files/{file_id}/content"))
                .header(header::RANGE, "bytes=2-5")
                .body(Body::empty())
                .expect("build range request"),
        )
        .await
        .expect("stream source range");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-cache"
    );
    let bytes = to_bytes(response.into_body(), 64)
        .await
        .expect("read source range");
    assert_eq!(&bytes[..], b"2345");

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/files/{file_id}/previews"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"web"}"#))
                .expect("build preview request"),
        )
        .await
        .expect("enqueue source preview");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("read preview response");
    let preview: serde_json::Value = serde_json::from_slice(&body).expect("preview JSON");
    assert_eq!(preview["status"], "queued");
    assert!(preview["id"].as_str().is_some());

    fs::remove_dir_all(root).expect("remove media fixture");
}
