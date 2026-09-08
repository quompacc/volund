#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(router: &Router, method: &str, route: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(route)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn concurrent_disable_blocks_new_scan_request() {
    race(false).await;
}

#[tokio::test]
async fn concurrent_disable_blocks_full_scan_promotion() {
    race(true).await;
}

async fn race(existing: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-scan-disable-race-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    sqlx::query("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('race','Race',$1)").bind(path.to_str().unwrap()).execute(&*db).await.unwrap();
    let route = "/api/v1/libraries/race/scans";
    if existing {
        assert_eq!(
            request(&router, "POST", route, json!({"full":false}))
                .await
                .0,
            StatusCode::ACCEPTED
        );
    }
    let before = scans(&db).await;
    let mut disabling = db.begin().await.unwrap();
    // Keep the old enabled=true version visible to the request's initial read.
    sqlx::query("SELECT id FROM volund.library_roots WHERE root_key='race' FOR UPDATE")
        .execute(&mut *disabling)
        .await
        .unwrap();
    sqlx::query("UPDATE volund.library_roots SET enabled=false WHERE root_key='race'")
        .execute(&mut *disabling)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM volund.scan_runs FOR UPDATE")
        .execute(&mut *disabling)
        .await
        .unwrap();
    let r = router.clone();
    let pending = tokio::spawn(async move {
        request(
            &r,
            "POST",
            route,
            json!({"full":true,"confirmation":"FULL SCAN race"}),
        )
        .await
    });
    wait_for_blocked(&db, 1).await;
    disabling.commit().await.unwrap();
    let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(scans(&db).await, before);
    assert_audits(&db, i64::from(existing)).await;
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        request(
            &router,
            "PATCH",
            "/api/v1/libraries/race",
            json!({"enabled":true,"expectedRevision":1,"confirmation":"UPDATE LIBRARY race"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, result) = request(
        &router,
        "POST",
        route,
        json!({"full":true,"confirmation":"FULL SCAN race"}),
    )
    .await;
    std::fs::remove_dir(path).unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(result["full"], true);
    let rows = scans(&db).await;
    assert_eq!(rows.as_array().unwrap().len(), 1);
    if existing {
        assert_eq!(rows[0]["public_id"], before[0]["public_id"]);
    }
    assert_audits(&db, i64::from(existing) + 1).await;
}

async fn scans(db: &sqlx::PgPool) -> Value {
    sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(scan) ORDER BY scan.id),'[]'::jsonb) FROM volund.scan_runs scan").fetch_one(db).await.unwrap()
}

async fn wait_for_blocked(db: &sqlx::PgPool, minimum: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND wait_event IN ('relation','transactionid','tuple') AND cardinality(pg_blocking_pids(pid))>0").fetch_one(db).await.unwrap();
            if blocked >= minimum { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn admitted_scan_commits_before_concurrent_disable() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-scan-admitted-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    sqlx::query("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('race','Race',$1)").bind(path.to_str().unwrap()).execute(&*db).await.unwrap();
    let mut barrier = db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.security_audit_events IN SHARE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let r = router.clone();
    let scanning = tokio::spawn(async move {
        request(
            &r,
            "POST",
            "/api/v1/libraries/race/scans",
            json!({"full":false}),
        )
        .await
    });
    wait_for_blocked(&db, 1).await;
    let disabling = tokio::spawn(async move {
        request(
            &router,
            "PATCH",
            "/api/v1/libraries/race",
            json!({"enabled":false,"expectedRevision":1,"confirmation":"UPDATE LIBRARY race"}),
        )
        .await
    });
    wait_for_blocked(&db, 2).await;
    let blocked_updates: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'UPDATE volund.library_roots SET %'").fetch_one(&*db).await.unwrap();
    let blocked_queries: Vec<String> = sqlx::query_scalar("SELECT query FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'").fetch_all(&*db).await.unwrap();
    assert_eq!(
        blocked_updates, 1,
        "deactivation must wait at the library UPDATE, not merely at its audit: {blocked_queries:?}"
    );
    let enabled: bool =
        sqlx::query_scalar("SELECT enabled FROM volund.library_roots WHERE root_key='race'")
            .fetch_one(&*db)
            .await
            .unwrap();
    assert!(enabled);
    barrier.rollback().await.unwrap();
    let (scan_status, scan_result) =
        tokio::time::timeout(std::time::Duration::from_secs(10), scanning)
            .await
            .unwrap()
            .unwrap();
    let (disabled_status, disabled) =
        tokio::time::timeout(std::time::Duration::from_secs(10), disabling)
            .await
            .unwrap()
            .unwrap();
    std::fs::remove_dir(path).unwrap();
    assert_eq!(scan_status, StatusCode::ACCEPTED);
    assert_eq!(disabled_status, StatusCode::OK);
    assert_eq!(disabled["enabled"], false);
    let rows = scans(&db).await;
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["public_id"], scan_result["id"]);
    assert_audits(&db, 1).await;
}

async fn assert_audits(db: &sqlx::PgPool, expected: i64) {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='library.scan.queued' AND outcome='success'").fetch_one(db).await.unwrap();
    assert_eq!(count, expected);
}
