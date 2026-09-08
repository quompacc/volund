#![cfg(target_os = "linux")]

use axum::http::StatusCode;
use volundd::{api, operations};

mod support;

use support::{authenticated_router, get_json, test_database};

#[tokio::test]
async fn owner_sees_sanitized_aggregate_health_and_failed_components_block_it() {
    let Some(pool) = test_database().await else {
        return;
    };
    for component in [
        "preview-worker",
        "scan-worker",
        "scheduler",
        "retention-worker",
        "import-cleanup",
        "backup",
    ] {
        operations::record_component(&pool, component, true)
            .await
            .expect("record successful component");
    }
    let public_router = api::router((*pool).clone());
    let (status, _) = get_json(&public_router, "/api/v1/operations/health").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let (status, healthy) = get_json(&router, "/api/v1/operations/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(healthy["state"], "healthy");
    assert_eq!(
        healthy["database"]["appliedMigrations"],
        volundd::database::EXPECTED_MIGRATIONS
    );
    assert_eq!(
        healthy["database"]["schemaTables"],
        volundd::database::EXPECTED_SCHEMA_TABLES
    );
    assert_eq!(healthy["workers"].as_array().map(Vec::len), Some(5));
    assert_eq!(healthy["backup"]["lastOutcome"], "success");
    assert!(healthy.get("error").is_none());

    operations::record_component(&pool, "scan-worker", false)
        .await
        .expect("record failed component");
    let (status, blocked) = get_json(&router, "/api/v1/operations/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["workers"][1]["lastOutcome"], "failed");
    assert_eq!(
        blocked["workers"][1]["reasons"][0],
        "component_last_run_failed"
    );
}

#[tokio::test]
async fn successful_heartbeats_are_write_throttled() {
    let Some(pool) = test_database().await else {
        return;
    };
    operations::record_component(&pool, "preview-worker", true)
        .await
        .expect("record first heartbeat");
    let first: String = sqlx::query_scalar(
        "SELECT updated_at::text FROM volund.operational_components WHERE component_key = 'preview-worker'",
    )
    .fetch_one(&*pool)
    .await
    .expect("load first timestamp");
    operations::record_component(&pool, "preview-worker", true)
        .await
        .expect("record throttled heartbeat");
    let second: String = sqlx::query_scalar(
        "SELECT updated_at::text FROM volund.operational_components WHERE component_key = 'preview-worker'",
    )
    .fetch_one(&*pool)
    .await
    .expect("load second timestamp");
    assert_eq!(first, second);
}
