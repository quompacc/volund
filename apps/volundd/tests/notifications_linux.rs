#![cfg(target_os = "linux")]
//! In-app notification center (ADR 0038): conditions stay until resolved,
//! events are unread until the account opens the center.
mod support;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use support::{authenticated_router, get_json, test_database};
use tower::ServiceExt;
use volundd::scanner;

fn find<'a>(page: &'a Value, key_prefix: &str) -> Vec<&'a Value> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["key"].as_str().unwrap().starts_with(key_prefix))
        .collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One instance walks through every notification source.
async fn notifications_derive_conditions_and_events_with_a_read_marker() {
    let Some(pool) = test_database().await else {
        return;
    };
    let library = std::env::temp_dir().join(format!("volund-notify-{}", std::process::id()));
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(library.join("bracket.step"), b"bracket").unwrap();
    scanner::register_root(&pool, "notify", "Notify", &library)
        .await
        .unwrap();
    scanner::scan_root(&pool, "notify", false).await.unwrap();
    sqlx::query("INSERT INTO volund.models (slug,name,kind) VALUES ('bracket','Bracket','part')")
        .execute(&*pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal) \
         SELECT model.id,source.id,'master-cad',true,1 FROM volund.models model, volund.source_files source",
    ).execute(&*pool).await.unwrap();
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;

    // A fresh instance never completed a backup: a condition for administrators.
    let (status, first) = get_json(&router, "/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let backup = find(&first, "backup:");
    assert_eq!(backup.len(), 1, "{first}");
    assert_eq!(
        (backup[0]["kind"].as_str(), backup[0]["unread"].as_bool()),
        (Some("condition"), Some(true))
    );

    // A failed conversion, a failed scan, and repeated login denials are events.
    sqlx::query(
        "INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,contract_version,\
         profile,status,started_at,finished_at,diagnostics) SELECT content_object_id,'volund-cad-convert','test',1,\
         'web','timed-out',now(),now(),'[]' FROM volund.source_files",
    ).execute(&*pool).await.unwrap();
    sqlx::query(
        "INSERT INTO volund.scan_runs (library_root_id,status,started_at,finished_at,error_message) \
         SELECT id,'failed',now(),now(),'unreadable' FROM volund.library_roots",
    ).execute(&*pool).await.unwrap();
    sqlx::query(
        "INSERT INTO volund.security_audit_events (action,outcome,target_type) \
         SELECT 'session.login','denied','user' FROM generate_series(1,6)",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let (_, page) = get_json(&router, "/api/v1/notifications").await;
    let conversion = find(&page, "conversion:");
    assert_eq!(conversion.len(), 1, "{page}");
    assert_eq!(
        conversion[0]["title"],
        "Konvertierung abgebrochen (Zeitlimit)"
    );
    assert_eq!(conversion[0]["view"], "model-problems");
    assert!(
        conversion[0]["message"]
            .as_str()
            .unwrap()
            .contains("Bracket")
    );
    assert_eq!(find(&page, "scan:").len(), 1);
    let login = find(&page, "security:session.login");
    assert_eq!(
        (login.len(), login[0]["severity"].as_str()),
        (1, Some("warning"))
    );
    let before = page["attention"].as_i64().unwrap();
    assert!(before >= 4, "{page}");

    // Opening the center reads events; conditions keep demanding attention.
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/notifications/seen")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (_, seen) = get_json(&router, "/api/v1/notifications").await;
    assert_eq!(find(&seen, "conversion:")[0]["unread"], false);
    assert_eq!(find(&seen, "backup:")[0]["unread"], true);
    let conditions = seen["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "condition")
        .count();
    assert_eq!(
        seen["attention"].as_i64(),
        Some(i64::try_from(conditions).unwrap())
    );
    assert!(seen["seenAtUnixMs"].is_i64());

    // Healthy backups clear the condition.
    volundd::operations::record_component(&pool, "backup", true)
        .await
        .unwrap();
    let (_, healthy) = get_json(&router, "/api/v1/notifications").await;
    assert!(find(&healthy, "backup:").is_empty(), "{healthy}");
    std::fs::remove_dir_all(library).unwrap();
}
