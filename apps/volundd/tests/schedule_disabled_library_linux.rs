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
async fn disabled_library_allows_stopping_but_not_enabling_its_schedule() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let root: String = sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('schedule_boundary','Schedule boundary','/var/tmp/unused-schedule-boundary') RETURNING public_id::text").fetch_one(&*db).await.unwrap();
    let mut input = json!({"libraryId":root,"name":"Boundary","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true,"confirmation":"APPLY SCHEDULE new"});
    let (status, created) = request(&router, "POST", "/api/v1/scan-schedules", input.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/scan-schedules/{id}");
    sqlx::query("UPDATE volund.library_roots SET enabled=false WHERE public_id::text=$1")
        .bind(&root)
        .execute(&*db)
        .await
        .unwrap();
    input["confirmation"] = json!(format!("APPLY SCHEDULE {id}"));
    input["expectedRevision"] = created["revision"].clone();
    input["enabled"] = json!(false);
    let (status, stopped) = request(&router, "PUT", &path, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["enabled"], false);
    assert_eq!(stopped["nextRunAtUnixMs"], Value::Null);
    let stale = request(&router, "PUT", &path, input.clone()).await;
    assert!(!stale.0.is_success());
    input["expectedRevision"] = stopped["revision"].clone();
    input["enabled"] = json!(true);
    assert!(
        !request(&router, "PUT", &path, input.clone())
            .await
            .0
            .is_success()
    );
    let (_, listed) = request(&router, "GET", "/api/v1/scan-schedules", json!({})).await;
    assert_eq!(listed[0]["enabled"], false);
    assert_eq!(listed[0]["revision"], stopped["revision"]);
    sqlx::query("UPDATE volund.library_roots SET enabled=true WHERE public_id::text=$1")
        .bind(&root)
        .execute(&*db)
        .await
        .unwrap();
    let (status, enabled) = request(&router, "PUT", &path, input).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled["enabled"], true);
    assert!(enabled["nextRunAtUnixMs"].is_number());
}
