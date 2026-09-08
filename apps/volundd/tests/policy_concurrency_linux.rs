#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(r: &Router, method: &str, path: &str, v: Value) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let b = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        if b.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&b).unwrap()
        },
    )
}

#[tokio::test]
async fn profile_writers_share_one_revision_and_one_success_audit() {
    race(false).await;
}

#[tokio::test]
async fn schedule_writers_share_one_revision_and_one_success_audit() {
    race(true).await;
}

async fn race(schedule: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let (route, table, action, mut input) = if schedule {
        let root: String = sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('race','Race','/var/tmp/unused-policy-race') RETURNING public_id::text").fetch_one(&*db).await.unwrap();
        (
            "scan-schedules",
            "scan_schedules",
            "scan-schedule.update",
            json!({"libraryId":root,"name":"Race","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true,"confirmation":"APPLY SCHEDULE new"}),
        )
    } else {
        (
            "conversion-profiles",
            "conversion_profiles",
            "conversion-profile.update",
            json!({"name":"Race","nativePreset":"web","linearDeflection":null,"angularDeflection":null,"enabled":true,"confirmation":"APPLY PROFILE new"}),
        )
    };
    let list_path = format!("/api/v1/{route}");
    let (status, created) = request(&router, "POST", &list_path, input.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("{list_path}/{id}");
    input["expectedRevision"] = created["revision"].clone();
    input["confirmation"] = json!(format!(
        "APPLY {} {id}",
        if schedule { "SCHEDULE" } else { "PROFILE" }
    ));
    let mut barrier = db.begin().await.unwrap();
    sqlx::query(&format!(
        "SELECT id FROM volund.{table} WHERE public_id::text=$1 FOR UPDATE"
    ))
    .bind(id)
    .execute(&mut *barrier)
    .await
    .unwrap();
    let mut tasks = Vec::new();
    for name in ["Writer A", "Writer B"] {
        let r = router.clone();
        let p = path.clone();
        let mut v = input.clone();
        v["name"] = json!(name);
        tasks.push(tokio::spawn(async move { request(&r, "PUT", &p, v).await }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND cardinality(pg_blocking_pids(pid))>0").fetch_one(&*db).await.unwrap();
            if waiting>=2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("both writers must reach the database lock barrier");
    barrier.rollback().await.unwrap();
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(outcomes.iter().filter(|v| v.0 == StatusCode::OK).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|v| v.0 == StatusCode::BAD_REQUEST)
            .count(),
        1
    );
    let winner = &outcomes.iter().find(|v| v.0 == StatusCode::OK).unwrap().1;
    assert_eq!(
        winner["revision"].as_i64(),
        Some(created["revision"].as_i64().unwrap() + 1)
    );
    let (_, listed) = request(&router, "GET", &list_path, json!({})).await;
    assert_eq!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id)
            .unwrap(),
        winner
    );
    let audits:i64=sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND target_public_id::text=$2 AND outcome='success'").bind(action).bind(id).fetch_one(&*db).await.unwrap();
    assert_eq!(audits, 1);
}
