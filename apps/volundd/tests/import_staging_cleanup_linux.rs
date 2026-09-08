#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    db: support::TestDatabase,
    router: Router,
    root: std::path::PathBuf,
    library_id: String,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let db = support::test_database().await?;
        let root = std::env::temp_dir().join(format!(
            "volund-review-probe-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("library")).unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        volundd::scanner::register_root(&db, "probe", "Review probe", &root.join("library"))
            .await
            .unwrap();
        let library_id = sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots")
            .fetch_one(&*db)
            .await
            .unwrap();
        let router = support::authenticated_router(
            &db,
            volundd::api::router_with_roots(db.clone(), root.join("derived"), root.join("web")),
        )
        .await;
        Some(Self {
            db,
            router,
            root,
            library_id,
        })
    }
    async fn upload(&self, duplicate: bool) -> String {
        let (status, draft) = post(
            &self.router,
            "/api/v1/imports/preview",
            json!({"sourceName":"Probe","entries": if duplicate { json!([{"path":"Package/probe.step","byteSize":4},{"path":"Package/copy.step","byteSize":4}]) } else { json!([{"path":"Package/probe.step","byteSize":4}]) }}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{draft}");
        let id = draft["id"].as_str().unwrap();

        let (status, body) = post(&self.router, &format!("/api/v1/imports/{id}/metadata"), json!({"modelName":"Review Probe","kind":"part","libraryRootId":self.library_id,"description":"","tags":[],"collectionIds":[]})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        for entry in draft["items"].as_array().unwrap() {
            let item = entry["id"].as_str().unwrap();
            let response = self
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/imports/{id}/items/{item}/content"))
                        .header("content-type", "application/octet-stream")
                        .header("content-length", "4")
                        .body(Body::from("AAAA"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        id.to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn committed_staging_is_retried_by_cleanup() {
    cleanup_retry(false).await;
}
#[tokio::test]
async fn committed_staging_is_retried_by_commit() {
    cleanup_retry(true).await;
}
async fn cleanup_retry(via_commit: bool) {
    use std::os::unix::fs::PermissionsExt;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload(false).await;
    let incoming = f.root.join("incoming");
    let staged = incoming.join(&id);
    assert!(staged.is_dir());
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    let blocked = volundd::import_cleanup::run(&f.db, &incoming)
        .await
        .unwrap();
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(blocked.status, "partial");
    assert!(!status.is_success(), "{result}");
    let remaining: u64 = std::fs::read_dir(&staged)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    let (storage_status, storage) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(storage_status, StatusCode::OK);

    assert_eq!(remaining, 4);
    assert_eq!(storage["uploadedBytes"], 4);
    assert_eq!(storage["reclaimableBytes"], 4);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM volund.import_drafts")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        "committed"
    );
    assert_eq!(
        last_error(&f, &id).await.as_deref(),
        Some("staging_cleanup_refused")
    );
    assert_eq!(reserve_ten_gib(&f.router).await, StatusCode::OK);
    assert_eq!(reserve_ten_gib(&f.router).await, StatusCode::BAD_REQUEST);
    if via_commit {
        let (retry_status, _) = post(
            &f.router,
            &format!("/api/v1/imports/{id}/commit"),
            json!({}),
        )
        .await;
        assert_eq!(retry_status, StatusCode::OK);
    } else {
        let cleanup = volundd::import_cleanup::run(&f.db, &incoming)
            .await
            .unwrap();
        assert_eq!(cleanup.cleaned_drafts, 1);
        assert_eq!(cleanup.cleaned_bytes, 4);
    }
    assert!(!staged.exists());
    let (_, storage) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(storage["uploadedBytes"], 0);
    assert_eq!(storage["reclaimableBytes"], 0);
    let (retry_status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(retry_status, StatusCode::OK);
    assert_eq!(
        volundd::import_cleanup::run(&f.db, &incoming)
            .await
            .unwrap()
            .cleaned_drafts,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.models")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        1
    );
    assert!(last_error(&f, &id).await.is_none());
    assert_eq!(reserve_ten_gib(&f.router).await, StatusCode::OK);
}
async fn last_error(f: &Fixture, id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT last_error_code FROM volund.import_drafts WHERE public_id::text=$1")
        .bind(id)
        .fetch_one(&*f.db)
        .await
        .unwrap()
}
async fn reserve_ten_gib(router: &Router) -> StatusCode {
    let entries: Vec<Value> = (0..5)
        .map(|i| json!({"path":format!("part-{i}.step"),"byteSize":2_i64*1024*1024*1024}))
        .collect();
    post(
        router,
        "/api/v1/imports/preview",
        json!({"sourceName":"Capacity","entries":entries}),
    )
    .await
    .0
}

#[tokio::test]
async fn committing_and_terminal_staging_remain_capacity_accounted() {
    for status in ["committing", "cancelled", "expired"] {
        capacity_for_state(status).await;
    }
}
async fn capacity_for_state(status: &str) {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload(false).await;
    assert_eq!(
        post(
            &f.router,
            &format!("/api/v1/imports/{id}/review"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    // Keep actual four-byte staging, but a larger former manifest reservation.
    sqlx::query("UPDATE volund.import_drafts SET total_bytes=1073741824 WHERE public_id::text=$1")
        .bind(&id)
        .execute(&*f.db)
        .await
        .unwrap();
    assert_eq!(reserve_ten_gib(&f.router).await, StatusCode::OK);
    sqlx::query(
        "UPDATE volund.import_drafts SET status=$2, \
            cancelled_at=CASE WHEN $2='cancelled' THEN now() ELSE NULL END, \
            expired_at=CASE WHEN $2='expired' THEN now() ELSE NULL END WHERE public_id::text=$1",
    )
    .bind(&id)
    .bind(status)
    .execute(&*f.db)
    .await
    .unwrap();
    assert_eq!(reserve_ten_gib(&f.router).await, StatusCode::BAD_REQUEST);
    if status == "committing" {
        return;
    }
    let entries: Vec<Value> = (0..5)
        .map(|i| json!({"path":format!("boundary-{i}.step"),"byteSize":2_i64*1024*1024*1024-if i==0 {4} else {0}}))
        .collect();
    assert_eq!(
        post(
            &f.router,
            "/api/v1/imports/preview",
            json!({"sourceName":"Boundary","entries":entries})
        )
        .await
        .0,
        StatusCode::OK
    );
    volundd::import_cleanup::run(&f.db, &f.root.join("incoming"))
        .await
        .unwrap();
    assert_eq!(
        post(
            &f.router,
            "/api/v1/imports/preview",
            json!({"sourceName":"Released","entries":[{"path":"released.step","byteSize":4}]})
        )
        .await
        .0,
        StatusCode::OK
    );
}
async fn post(router: &Router, uri: &str, value: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}
