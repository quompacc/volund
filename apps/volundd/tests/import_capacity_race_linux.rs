#![cfg(target_os = "linux")]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn concurrent_previews_cannot_overbook_capacity() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    // Stop persistence after admission. Old code lets all three requests pass
    // admission; fixed code serializes them at the transaction advisory lock.
    let mut blocker = db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.import_drafts IN SHARE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..3 {
        tasks.push(tokio::spawn(preview(router.clone())));
    }
    let reached = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE NOT granted AND \
                 database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND \
                 ((relation='volund.import_drafts'::regclass AND mode='RowExclusiveLock') OR \
                 (locktype='advisory' AND classid=860756368 AND objid=3 AND objsubid=2))",
            )
            .fetch_one(&*db)
            .await
            .unwrap();
            if waiting == 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    blocker.rollback().await.unwrap();
    let mut accepted = 0;
    let mut rejected = 0;
    for task in tasks {
        match tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
        {
            StatusCode::OK => accepted += 1,
            StatusCode::BAD_REQUEST => rejected += 1,
            status => panic!("unexpected preview status: {status}"),
        }
    }
    assert!(
        reached.is_ok(),
        "three overlapping requests must reach database locks"
    );
    assert_eq!((accepted, rejected), (2, 1));
    let (_, storage) = support::get_json(&router, "/api/v1/imports/storage").await;
    assert_eq!(storage["reservedBytes"], 20_i64 * 1024 * 1024 * 1024);
    assert_eq!(storage["capacityBytes"], storage["reservedBytes"]);
    assert_eq!(storage["uploadedBytes"], 0);
    let drafts: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.import_drafts")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert_eq!(drafts, 2);
    // A rejected admission must release its lock as well.
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), preview(router))
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST
    );
}

async fn preview(router: Router) -> StatusCode {
    let entries: Vec<Value> = (0..5)
        .map(|i| json!({"path":format!("part-{i}.step"),"byteSize":2_i64*1024*1024*1024}))
        .collect();
    router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/imports/preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"sourceName":"Concurrent","entries":entries}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
