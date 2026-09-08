#![cfg(target_os = "linux")]

use std::env;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::{api, preview_pipeline, scanner};

mod support;

use support::{authenticated_router, test_database};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn fixture_root() -> PathBuf {
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("volund-preview-{unique}"))
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One end-to-end lifecycle with shared disposable state.
async fn preview_request_is_idempotent_processed_and_visible_through_api() {
    let Some(pool) = test_database().await else {
        return;
    };
    let runner = env::var("VOLUND_TEST_RUNNER").expect("VOLUND_TEST_RUNNER is set");
    let root = fixture_root();
    let library = root.join("library");
    fs::create_dir_all(&library).expect("create library fixture");
    fs::write(library.join("part.step"), b"pipeline CAD fixture").expect("write CAD fixture");
    let root_key = format!("preview_{}", std::process::id());
    scanner::register_root(&pool, &root_key, "Preview Test", &library)
        .await
        .expect("register preview root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan preview root");
    let file_id: String = sqlx::query_scalar(
        "SELECT source.public_id::text FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id = source.library_root_id \
         WHERE root.root_key = $1",
    )
    .bind(&root_key)
    .fetch_one(&*pool)
    .await
    .expect("load file id");
    let first = preview_pipeline::enqueue(&pool, &file_id, "web")
        .await
        .expect("enqueue preview");
    let duplicate = preview_pipeline::enqueue(&pool, &file_id, "web")
        .await
        .expect("reuse queued preview");
    assert_eq!(first, duplicate);

    let converter = root.join("fixture-converter.sh");
    fs::write(
        &converter,
        "#!/bin/sh\nset -eu\nout=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--output\" ]; then out=$2; shift 2; else shift; fi\ndone\nmkdir -p \"$out\"\nprintf glb > \"$out/preview.glb\"\nprintf png > \"$out/thumbnail.png\"\nprintf '{}' > \"$out/assembly.json\"\nprintf '[]' > \"$out/diagnostics.json\"\nprintf '{}' > \"$out/result.json\"\n",
    )
    .expect("write converter fixture");
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o750))
        .expect("make converter executable");
    let config = preview_pipeline::PreviewWorkerConfig::new(
        runner.into(),
        converter,
        root.join("derived"),
        root.join("scratch"),
        30,
    )
    .expect("worker config");
    let report = preview_pipeline::process_next(&pool, &config)
        .await
        .expect("process preview")
        .expect("one queued preview");
    assert_eq!(report.id, first.id);
    assert_eq!(report.status, "ready");
    assert!(
        preview_pipeline::process_next(&pool, &config)
            .await
            .expect("idle worker")
            .is_none()
    );
    let artifact_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.derived_artifacts artifact \
         JOIN volund.conversion_runs run ON run.id = artifact.conversion_run_id \
         WHERE run.public_id::text = $1",
    )
    .bind(&first.id)
    .fetch_one(&*pool)
    .await
    .expect("count artifacts");
    assert_eq!(artifact_count, 5);
    assert_eq!(
        preview_pipeline::enqueue(&pool, &file_id, "web")
            .await
            .expect("reuse ready preview")
            .id,
        first.id
    );

    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), config.derived_root.clone(), root.join("web")),
    )
    .await;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/files/{file_id}/previews"))
                .body(Body::empty())
                .expect("build API request"),
        )
        .await
        .expect("route API request");
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read API body");
    let json: Value = serde_json::from_slice(&body).expect("parse API body");
    assert_eq!(json["items"][0]["status"], "ready");
    assert_eq!(
        json["items"][0]["artifacts"]
            .as_array()
            .expect("artifacts")
            .len(),
        5
    );

    let artifact_uri = format!("/api/v1/previews/{}/artifacts/preview-glb", first.id);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&artifact_uri)
                .header(header::RANGE, "bytes=1-2")
                .body(Body::empty())
                .expect("build range request"),
        )
        .await
        .expect("route range request");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 1-2/3");
    assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "model/gltf-binary"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let etag = response.headers()[header::ETAG]
        .to_str()
        .expect("etag")
        .to_owned();
    assert!(etag.starts_with('"'));
    assert_eq!(
        to_bytes(response.into_body(), 1024)
            .await
            .expect("read range body"),
        "lb"
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&artifact_uri)
                .header(header::RANGE, "bytes=99-")
                .body(Body::empty())
                .expect("build invalid range request"),
        )
        .await
        .expect("route invalid range request");
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */3");

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&artifact_uri)
                .header(header::IF_NONE_MATCH, etag)
                .body(Body::empty())
                .expect("build conditional request"),
        )
        .await
        .expect("route conditional request");
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        to_bytes(response.into_body(), 1024)
            .await
            .expect("read empty conditional body")
            .len(),
        0
    );

    let outside = root.join("outside.glb");
    fs::write(&outside, b"secret").expect("write outside artifact");
    symlink(&root, config.derived_root.join("escape")).expect("create escape symlink");
    sqlx::query(
        "UPDATE volund.derived_artifacts SET relative_path = 'escape/outside.glb' \
         WHERE conversion_run_id = (SELECT id FROM volund.conversion_runs \
         WHERE public_id::text = $1) AND artifact_kind = 'preview-glb'",
    )
    .bind(&first.id)
    .execute(&*pool)
    .await
    .expect("redirect catalog fixture through symlink");
    let response = router
        .oneshot(
            Request::builder()
                .uri(&artifact_uri)
                .body(Body::empty())
                .expect("build escape request"),
        )
        .await
        .expect("route escape request");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    fs::remove_dir_all(root).expect("remove preview fixture");
}

#[tokio::test]
async fn running_preview_cancellation_stops_publication_and_wins_completion() {
    let Some(pool) = test_database().await else {
        return;
    };
    let root = fixture_root();
    let library = root.join("library");
    fs::create_dir_all(&library).expect("create cancellation library");
    fs::write(library.join("cancel.step"), b"cancel preview fixture").expect("write fixture");
    let root_key = format!("cancel_{}", std::process::id());
    scanner::register_root(&pool, &root_key, "Cancellation Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan root");
    let file_id: String = sqlx::query_scalar(
        "SELECT source.public_id::text FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id = source.library_root_id WHERE root.root_key = $1",
    )
    .bind(&root_key)
    .fetch_one(&*pool)
    .await
    .expect("load cancellation file");
    let job = preview_pipeline::enqueue(&pool, &file_id, "web")
        .await
        .expect("enqueue preview");
    let slow_runner = root.join("slow-runner.sh");
    fs::write(&slow_runner, "#!/bin/sh\nsleep 10\nexit 1\n").expect("write slow runner");
    fs::set_permissions(&slow_runner, fs::Permissions::from_mode(0o750)).expect("chmod runner");
    let config = preview_pipeline::PreviewWorkerConfig::new(
        slow_runner,
        root.join("unused-converter"),
        root.join("derived"),
        root.join("scratch"),
        30,
    )
    .expect("cancellation worker config");
    let worker_pool = (*pool).clone();
    let worker =
        tokio::spawn(async move { preview_pipeline::process_next(&worker_pool, &config).await });
    for _ in 0..50 {
        let status: String = sqlx::query_scalar(
            "SELECT status FROM volund.conversion_runs WHERE public_id::text = $1",
        )
        .bind(&job.id)
        .fetch_one(&*pool)
        .await
        .expect("poll conversion status");
        if status == "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/jobs/conversion/{}/cancel", job.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"confirmation": format!("CANCEL {}", job.id)}).to_string(),
                ))
                .expect("build cancellation"),
        )
        .await
        .expect("cancel response");
    assert_eq!(response.status(), StatusCode::OK);
    let report = worker
        .await
        .expect("worker task")
        .expect("worker result")
        .expect("claimed conversion");
    assert_eq!(report.status, "cancelled");
    let (status, artifacts): (String, i64) = sqlx::query_as(
        "SELECT run.status, count(artifact.id) FROM volund.conversion_runs run \
         LEFT JOIN volund.derived_artifacts artifact ON artifact.conversion_run_id = run.id \
         WHERE run.public_id::text = $1 GROUP BY run.id",
    )
    .bind(&job.id)
    .fetch_one(&*pool)
    .await
    .expect("load cancelled conversion");
    assert_eq!(status, "cancelled");
    assert_eq!(artifacts, 0);
    fs::remove_dir_all(root).expect("remove cancellation preview fixture");
}
