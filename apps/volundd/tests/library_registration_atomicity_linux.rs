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

fn directory() -> std::path::PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-registration-atomicity-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    path
}

#[tokio::test]
async fn concurrent_duplicate_keys_have_one_winner() {
    registration_race(true).await;
}

#[tokio::test]
async fn concurrent_duplicate_paths_have_one_winner() {
    registration_race(false).await;
}

async fn registration_race(same_key: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let first = directory();
    let second = directory();
    let mut barrier = db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.library_roots IN SHARE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for (key, path, name) in [
        ("first", &first, "First"),
        (
            if same_key { "first" } else { "second" },
            if same_key { &second } else { &first },
            "Second",
        ),
    ] {
        let router = router.clone();
        let input =
            json!({"key":key,"path":path,"name":name,"confirmation":format!("ADD LIBRARY {key}")});
        tasks.push(tokio::spawn(async move {
            request(&router, "POST", "/api/v1/libraries", input).await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND wait_event IN ('relation','transactionid','tuple') AND cardinality(pg_blocking_pids(pid))>0").fetch_one(&*db).await.unwrap();
            if waiting >= 2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    barrier.rollback().await.unwrap();
    let mut winners = Vec::new();
    let mut conflicts = 0;
    for task in tasks {
        let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        match status {
            StatusCode::OK => winners.push(body),
            StatusCode::CONFLICT => conflicts += 1,
            _ => panic!("unexpected registration response: {status} {body}"),
        }
    }
    std::fs::remove_dir(first).unwrap();
    std::fs::remove_dir(second).unwrap();
    assert_eq!(conflicts, 1);
    assert_eq!(winners.len(), 1);
    let rows = library_snapshot(&db).await;
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["public_id"], winners[0]["id"]);
    assert_eq!(rows[0]["display_name"], winners[0]["name"]);
    assert_eq!(rows[0]["filesystem_path"], winners[0]["filesystemPath"]);
    let audits: Vec<String> = sqlx::query_scalar("SELECT target_public_id::text FROM volund.security_audit_events WHERE action='library.create' AND outcome='success'").fetch_all(&*db).await.unwrap();
    assert_eq!(audits, vec![winners[0]["id"].as_str().unwrap()]);
}

#[tokio::test]
async fn failed_registration_audit_rolls_back_and_can_retry() {
    for deferred in [false, true] {
        audit_failure(false, deferred).await;
    }
}

#[tokio::test]
async fn failed_edit_audit_preserves_library_and_can_retry() {
    for deferred in [false, true] {
        audit_failure(true, deferred).await;
    }
}

async fn library_snapshot(db: &sqlx::PgPool) -> Value {
    sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(root) ORDER BY root.id),'[]'::jsonb) FROM volund.library_roots root").fetch_one(db).await.unwrap()
}

async fn audit_failure(edit: bool, deferred: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let path = directory();
    let create =
        json!({"key":"atomic","name":"Before","path":path,"confirmation":"ADD LIBRARY atomic"});
    if edit {
        assert_eq!(
            request(&router, "POST", "/api/v1/libraries", create.clone())
                .await
                .0,
            StatusCode::OK
        );
    }
    let before = library_snapshot(&db).await;
    let (method, route, input, action) = if edit {
        (
            "PATCH",
            "/api/v1/libraries/atomic",
            json!({"name":"After","enabled":false,"expectedRevision":1,"confirmation":"UPDATE LIBRARY atomic"}),
            "library.update",
        )
    } else {
        ("POST", "/api/v1/libraries", create, "library.create")
    };
    sqlx::query(&format!("CREATE FUNCTION volund.reject_registration_audit_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='{action}' THEN RAISE EXCEPTION 'injected registration audit failure'; END IF; RETURN NEW; END $$")).execute(&*db).await.unwrap();
    let trigger = if deferred {
        "CREATE CONSTRAINT TRIGGER reject_registration_audit_test AFTER INSERT ON volund.security_audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION volund.reject_registration_audit_test()"
    } else {
        "CREATE TRIGGER reject_registration_audit_test BEFORE INSERT ON volund.security_audit_events FOR EACH ROW EXECUTE FUNCTION volund.reject_registration_audit_test()"
    };
    sqlx::query(trigger).execute(&*db).await.unwrap();
    let (status, _) = request(&router, method, route, input.clone()).await;
    sqlx::query("DROP TRIGGER reject_registration_audit_test ON volund.security_audit_events")
        .execute(&*db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.reject_registration_audit_test()")
        .execute(&*db)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(library_snapshot(&db).await, before);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND outcome='success'",
    )
    .bind(action)
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(audits, 0);
    let (status, retried) = request(&router, method, route, input).await;
    std::fs::remove_dir(path).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retried["enabled"], !edit);
    assert_eq!(retried["name"], if edit { "After" } else { "Before" });
    if edit {
        assert_eq!(retried["id"], before[0]["public_id"]);
    }
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND outcome='success'",
    )
    .bind(action)
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}
