#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(router: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
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
async fn stale_profile_retirement_is_rejected() {
    stale_confirmation(false).await;
}

#[tokio::test]
async fn stale_schedule_deletion_is_rejected() {
    stale_confirmation(true).await;
}

async fn stale_confirmation(schedule: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let (route, kind, terminal, mut input) = if schedule {
        let root: String = sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('revision','Revision','/var/tmp/unused-policy-revision') RETURNING public_id::text").fetch_one(&*db).await.unwrap();
        (
            "scan-schedules",
            "SCHEDULE",
            "DELETE SCHEDULE",
            json!({"libraryId":root,"name":"Before","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true}),
        )
    } else {
        (
            "conversion-profiles",
            "PROFILE",
            "RETIRE",
            json!({"name":"Before","nativePreset":"web","linearDeflection":null,"angularDeflection":null,"enabled":true}),
        )
    };
    let list_path = format!("/api/v1/{route}");
    input["confirmation"] = json!(format!("APPLY {kind} new"));
    let (status, created) = request(&router, "POST", &list_path, input.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("{list_path}/{id}");
    input["name"] = json!("After");
    input["expectedRevision"] = created["revision"].clone();
    input["confirmation"] = json!(format!("APPLY {kind} {id}"));
    let (status, updated) = request(&router, "PUT", &path, input).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["revision"], 2);
    let confirmation = format!("{terminal} {id}");
    for revision in [created["revision"].clone(), json!(0)] {
        let (status, body) = request(
            &router,
            "DELETE",
            &path,
            json!({"confirmation":confirmation,"expectedRevision":revision}),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "stale confirmation: {body}"
        );
    }
    assert_eq!(
        request(
            &router,
            "DELETE",
            &path,
            json!({"confirmation":confirmation})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (_, listed) = request(&router, "GET", &list_path, json!({})).await;
    assert_eq!(
        listed.as_array().unwrap().iter().find(|v| v["id"] == id),
        Some(&updated)
    );
    let action = if schedule {
        "scan-schedule.delete"
    } else {
        "conversion-profile.retire"
    };
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND target_public_id::text=$2 AND outcome='success'").bind(action).bind(id).fetch_one(&*db).await.unwrap();
    assert_eq!(count, 0);
    let (status, result) = request(
        &router,
        "DELETE",
        &path,
        json!({"confirmation":confirmation,"expectedRevision":updated["revision"]}),
    )
    .await;
    assert_eq!(
        status,
        if schedule {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::OK
        }
    );
    if !schedule {
        assert_eq!(result["enabled"], false);
        assert_eq!(result["revision"], 3);
    }
    let (_, listed) = request(&router, "GET", &list_path, json!({})).await;
    let remaining = listed.as_array().unwrap().iter().find(|v| v["id"] == id);
    if schedule {
        assert!(remaining.is_none());
    } else {
        assert_eq!(remaining, Some(&result));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND target_public_id::text=$2 AND outcome='success'").bind(action).bind(id).fetch_one(&*db).await.unwrap();
    assert_eq!(count, 1);
}
