#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn scan(router: &Router, full: bool) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/libraries/atomic/scans")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"full":full,"confirmation":"FULL SCAN atomic"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn failed_audit_does_not_leave_a_queued_scan() {
    for deferred in [false, true] {
        audit_failure(false, deferred).await;
    }
}

#[tokio::test]
async fn failed_audit_does_not_upgrade_an_existing_scan() {
    for deferred in [false, true] {
        audit_failure(true, deferred).await;
    }
}

async fn inject_failure(db: &sqlx::PgPool, deferred: bool) {
    sqlx::query("CREATE FUNCTION volund.reject_scan_audit_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='library.scan.queued' THEN RAISE EXCEPTION 'injected scan audit failure'; END IF; RETURN NEW; END $$").execute(db).await.unwrap();
    let query = if deferred {
        "CREATE CONSTRAINT TRIGGER reject_scan_audit_test AFTER INSERT ON volund.security_audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION volund.reject_scan_audit_test()"
    } else {
        "CREATE TRIGGER reject_scan_audit_test BEFORE INSERT ON volund.security_audit_events FOR EACH ROW EXECUTE FUNCTION volund.reject_scan_audit_test()"
    };
    sqlx::query(query).execute(db).await.unwrap();
}

async fn audit_failure(existing: bool, deferred: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-scan-atomicity-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    sqlx::query("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('atomic','Atomic',$1)").bind(path.to_str().unwrap()).execute(&*db).await.unwrap();
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let original = if existing {
        let (status, result) = scan(&router, false).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        Some(result)
    } else {
        None
    };
    inject_failure(&db, deferred).await;
    let (status, _) = scan(&router, true).await;
    // Remove the fault before assertions, including on the intentionally red baseline.
    sqlx::query("DROP TRIGGER reject_scan_audit_test ON volund.security_audit_events")
        .execute(&*db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.reject_scan_audit_test()")
        .execute(&*db)
        .await
        .unwrap();
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT public_id::text, full_scan FROM volund.scan_runs ORDER BY id")
            .fetch_all(&*db)
            .await
            .unwrap();
    if let Some(original) = &original {
        assert_eq!(
            rows,
            vec![(original["id"].as_str().unwrap().to_owned(), false)],
            "failed request must not promote the existing scan"
        );
    } else {
        assert!(
            rows.is_empty(),
            "failed request must not leave queued work: {rows:?}"
        );
    }
    assert_audits(&db, i64::from(existing)).await;
    std::fs::create_dir(&path).unwrap();
    let (status, retried) = scan(&router, true).await;
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(retried["full"], true);
    if let Some(original) = original {
        assert_eq!(retried["id"], original["id"]);
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.scan_runs WHERE full_scan AND status='queued'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_audits(&db, i64::from(existing) + 1).await;
}

async fn assert_audits(db: &sqlx::PgPool, expected: i64) {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='library.scan.queued' AND outcome='success'").fetch_one(db).await.unwrap();
    assert_eq!(count, expected);
}
