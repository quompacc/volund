#![cfg(target_os = "linux")]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(router: &Router, method: &str, route: &str, input: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(route)
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn concurrent_name_and_activation_require_retry_with_current_revision() {
    metadata_race(false).await;
}

#[tokio::test]
async fn concurrent_names_have_one_winner_and_one_revision_conflict() {
    metadata_race(true).await;
}

#[tokio::test]
async fn missing_invalid_stale_and_unknown_library_revisions_do_not_mutate() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    sqlx::query("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('metadata','Before','/tmp')")
        .execute(&*db).await.unwrap();
    for (revision, expected) in [
        (None, StatusCode::UNPROCESSABLE_ENTITY),
        (Some(0), StatusCode::BAD_REQUEST),
        (Some(-1), StatusCode::BAD_REQUEST),
        (Some(2), StatusCode::CONFLICT),
    ] {
        let mut input = json!({"name":"After","confirmation":"UPDATE LIBRARY metadata"});
        if let Some(revision) = revision {
            input["expectedRevision"] = json!(revision);
        }
        assert_eq!(
            request(&router, "PATCH", "/api/v1/libraries/metadata", input)
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        request(
            &router,
            "PATCH",
            "/api/v1/libraries/unknown",
            json!({"expectedRevision":1,"name":"After","confirmation":"UPDATE LIBRARY unknown"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let row: (String, i64) = sqlx::query_as(
        "SELECT display_name,revision FROM volund.library_roots WHERE root_key='metadata'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(row, ("Before".into(), 1));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='library.update' AND outcome='success'").fetch_one(&*db).await.unwrap();
    assert_eq!(count, 0);
}

#[allow(clippy::too_many_lines)] // One controlled race, durable-state check and revision retry.
async fn metadata_race(same_field: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-metadata-race-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    let (status, created) = request(
        &router,
        "POST",
        "/api/v1/libraries",
        json!({"key":"metadata","name":"Before","path":path,"confirmation":"ADD LIBRARY metadata"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let before: Value = sqlx::query_scalar("SELECT to_jsonb(root) - ARRAY['display_name','enabled','updated_at','updated_by_user_id','revision'] FROM volund.library_roots root WHERE root_key='metadata'")
        .fetch_one(&*db).await.unwrap();

    let mut barrier = db.begin().await.unwrap();
    let barrier_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("LOCK TABLE volund.library_roots IN SHARE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for mut input in [
        json!({"name":"First"}),
        if same_field {
            json!({"name":"Second"})
        } else {
            json!({"enabled":false})
        },
    ] {
        input["confirmation"] = json!("UPDATE LIBRARY metadata");
        input["expectedRevision"] = json!(1);
        let router = router.clone();
        tasks.push(tokio::spawn(async move {
            request(&router, "PATCH", "/api/v1/libraries/metadata", input).await
        }));
    }
    // Count only product mutations directly waiting on this barrier, never the
    // advisory locks used by other integration tests for fixture isolation.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND wait_event IN ('relation','transactionid','tuple') AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'UPDATE volund.library_roots SET%'")
                .bind(barrier_pid).fetch_one(&*db).await.unwrap();
            if waiting == 2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    barrier.rollback().await.unwrap();
    let mut successes = 0;
    let mut conflicts = 0;
    let mut winner = Value::Null;
    for task in tasks {
        let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        match status {
            StatusCode::OK => {
                successes += 1;
                assert_eq!(body["id"], created["id"]);
                winner = body;
            }
            StatusCode::CONFLICT => conflicts += 1,
            _ => panic!("unexpected response {status}: {body}"),
        }
    }
    assert_eq!((successes, conflicts), (1, 1));
    let after: Value = sqlx::query_scalar("SELECT to_jsonb(root) - ARRAY['display_name','enabled','updated_at','updated_by_user_id','revision'] FROM volund.library_roots root WHERE root_key='metadata'")
        .fetch_one(&*db).await.unwrap();
    assert_eq!(after, before, "stable library fields must not change");
    assert_final_metadata(&db, created["id"].as_str().unwrap(), &winner).await;
    let input = json!({"name":"Retried","enabled":false,"expectedRevision":1,"confirmation":"UPDATE LIBRARY metadata"});
    assert_eq!(
        request(
            &router,
            "PATCH",
            "/api/v1/libraries/metadata",
            input.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_final_metadata(&db, created["id"].as_str().unwrap(), &winner).await;
    let mut retry = input;
    retry["expectedRevision"] = json!(2);
    let (status, result) = request(&router, "PATCH", "/api/v1/libraries/metadata", retry).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["revision"], 3);
    assert_eq!(result["name"], "Retried");
    assert_eq!(result["enabled"], false);
    std::fs::remove_dir(path).unwrap();
}

async fn assert_final_metadata(db: &sqlx::PgPool, public_id: &str, winner: &Value) {
    let (name, enabled, revision): (String, bool, i64) = sqlx::query_as(
        "SELECT display_name,enabled,revision FROM volund.library_roots WHERE root_key='metadata'",
    )
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!(json!(name), winner["name"]);
    assert_eq!(json!(enabled), winner["enabled"]);
    assert_eq!(revision, 2);
    assert_eq!(winner["revision"], 2);
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='library.update' AND outcome='success' AND target_public_id::text=$1")
        .bind(public_id).fetch_one(db).await.unwrap();
    assert_eq!(audits, 1);
}
