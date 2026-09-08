#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;

use support::{authenticated_router, get_json, test_database};

#[tokio::test]
async fn running_conversion_does_not_invent_measured_progress() {
    let Some(pool) = test_database().await else {
        return;
    };
    let id = conversion_fixture(&pool).await;
    sqlx::query("UPDATE volund.conversion_runs SET status='running',finished_at=NULL WHERE public_id::text=$1")
        .bind(&id).execute(&*pool).await.unwrap();
    let running = volundd::job_admin::get(&pool, "conversion", &id)
        .await
        .unwrap();
    assert_eq!(running.status, "running");
    assert_eq!(running.progress_current, 0);
    assert_eq!(running.progress_total, None);
    sqlx::query("UPDATE volund.conversion_runs SET status='ready',finished_at=now() WHERE public_id::text=$1")
        .bind(&id).execute(&*pool).await.unwrap();
    let ready = volundd::job_admin::get(&pool, "conversion", &id)
        .await
        .unwrap();
    assert_eq!(
        (ready.progress_current, ready.progress_total),
        (100, Some(100))
    );
}

#[tokio::test]
async fn jobs_are_unified_filtered_confirmed_idempotent_and_audited() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_id, root_key, root_path) = root_fixture(&pool, "jobs").await;
    let queued_id = scan_fixture(&pool, root_id, "queued").await;
    let failed_id = scan_fixture(&pool, root_id, "failed").await;
    let conversion_id = conversion_fixture(&pool).await;
    let public = api::router((*pool).clone());
    assert_eq!(
        get_json(&public, "/api/v1/jobs").await.0,
        StatusCode::UNAUTHORIZED
    );
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;

    let (status, page) = get_json(&router, "/api/v1/jobs?limit=2&offset=0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(2));
    let (status, conversions) =
        get_json(&router, "/api/v1/jobs?kind=conversion&status=failed").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conversions["items"][0]["id"], conversion_id);
    assert!(
        conversions["items"][0]["diagnostic"]
            .as_str()
            .map(str::len)
            .unwrap_or_default()
            < 160
    );

    let (status, _) = action(&router, "scan", &failed_id, "retry", "wrong").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, cancelled) = action(
        &router,
        "scan",
        &queued_id,
        "cancel",
        &format!("CANCEL {queued_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    let (status, repeated) = action(
        &router,
        "scan",
        &queued_id,
        "cancel",
        &format!("CANCEL {queued_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["id"], queued_id);

    let confirmation = format!("RETRY {failed_id}");
    let (status, retry) = action(&router, "scan", &failed_id, "retry", &confirmation).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry["attempt"], 2);
    assert_eq!(retry["retryOfId"], failed_id);
    let (status, duplicate) = action(&router, "scan", &failed_id, "retry", &confirmation).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["id"], retry["id"]);

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action IN ('job.cancel', 'job.retry')",
    )
    .fetch_one(&*pool)
    .await
    .expect("count job audits");
    assert_eq!(audited, 5);
    let denied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action='job.retry' AND outcome='denied'",
    )
    .fetch_one(&*pool)
    .await
    .expect("count denied job audits");
    assert_eq!(denied, 1);
    let confirmation = format!("RETRY {conversion_id}");
    let (status, retry) = action(
        &router,
        "conversion",
        &conversion_id,
        "retry",
        &confirmation,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry["status"], "queued");
    assert_eq!(retry["attempt"], 2);
    assert_eq!(retry["retryOfId"], conversion_id);
    let (status, duplicate) = action(
        &router,
        "conversion",
        &conversion_id,
        "retry",
        &confirmation,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["id"], retry["id"]);
    fs::remove_dir_all(root_path).expect("remove job fixture");
    let _ = root_key;
}

#[tokio::test]
async fn a_running_scan_honors_a_concurrent_cancellation_before_catalog_changes() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_id, root_key, root_path) = root_fixture(&pool, "cancel").await;
    fs::write(
        root_path.join("never-indexed.step"),
        b"ISO-10303-21;ENDSEC;",
    )
    .expect("write scan fixture");
    let scan_id = scan_fixture(&pool, root_id, "running").await;
    let database_id: i64 =
        sqlx::query_scalar("SELECT id FROM volund.scan_runs WHERE public_id::text = $1")
            .bind(&scan_id)
            .fetch_one(&*pool)
            .await
            .expect("load scan database ID");
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let (status, requested) = action(
        &router,
        "scan",
        &scan_id,
        "cancel",
        &format!("CANCEL {scan_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requested["status"], "running");
    assert!(
        requested["cancellationRequestedAtUnixMs"]
            .as_i64()
            .is_some()
    );
    assert_eq!(
        scanner::scan_queued(&pool, database_id, &root_key, false)
            .await
            .expect_err("cancelled scan must not run"),
        "scan cancellation requested"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM volund.scan_runs WHERE id = $1")
        .bind(database_id)
        .fetch_one(&*pool)
        .await
        .expect("load cancelled status");
    let source_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.source_files WHERE library_root_id = $1")
            .bind(root_id)
            .fetch_one(&*pool)
            .await
            .expect("count unchanged catalog");
    assert_eq!(status, "cancelled");
    assert_eq!(source_count, 0);
    fs::remove_dir_all(root_path).expect("remove cancellation fixture");
}

async fn root_fixture(pool: &sqlx::PgPool, label: &str) -> (i64, String, std::path::PathBuf) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let key = format!("{label}_{suffix}");
    let path = std::env::temp_dir().join(&key);
    fs::create_dir(&path).expect("create library fixture");
    let id = sqlx::query_scalar(
        "INSERT INTO volund.library_roots (root_key, display_name, filesystem_path) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&key)
    .bind(format!("Job {label}"))
    .bind(path.to_string_lossy().as_ref())
    .fetch_one(pool)
    .await
    .expect("insert root");
    (id, key, path)
}

async fn scan_fixture(pool: &sqlx::PgPool, root_id: i64, status: &str) -> String {
    sqlx::query_scalar(
        "INSERT INTO volund.scan_runs (library_root_id, status, finished_at, error_message) \
         VALUES ($1, $2, CASE WHEN $2 IN ('failed','completed','cancelled') THEN now() END, \
         CASE WHEN $2 = 'failed' THEN 'private /srv/path diagnostic' END) RETURNING public_id::text",
    )
    .bind(root_id)
    .bind(status)
    .fetch_one(pool)
    .await
    .expect("insert scan")
}

async fn conversion_fixture(pool: &sqlx::PgPool) -> String {
    let hash = format!(
        "{:064x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let content_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.content_objects (sha256, byte_size, detected_format) VALUES ($1, 1, 'step') RETURNING id",
    )
    .bind(hash)
    .fetch_one(pool)
    .await
    .expect("insert content");
    sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs (content_object_id, converter_name, converter_version, \
         contract_version, profile, status, started_at, finished_at, diagnostics) \
         VALUES ($1, 'volund-cad-convert', 'test', 1, 'web', 'failed', now(), now(), \
         '[{\"message\":\"secret raw output\"}]') RETURNING public_id::text",
    )
    .bind(content_id)
    .fetch_one(pool)
    .await
    .expect("insert conversion")
}

async fn action(
    router: &axum::Router,
    kind: &str,
    id: &str,
    action: &str,
    confirmation: &str,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/jobs/{kind}/{id}/{action}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"confirmation": confirmation}).to_string(),
                ))
                .expect("build job action"),
        )
        .await
        .expect("job action response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read job action");
    (
        status,
        serde_json::from_slice(&bytes).expect("parse job action"),
    )
}
