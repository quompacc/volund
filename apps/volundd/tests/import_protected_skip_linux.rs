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
async fn selected_primary_cannot_be_skipped() {
    protected_skip(false).await;
}
#[tokio::test]
async fn selected_thumbnail_cannot_be_skipped() {
    protected_skip(true).await;
}
async fn protected_skip(thumbnail: bool) {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload(false).await;
    let (_, initial) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    let item = initial["items"][0]["id"].as_str().unwrap();
    assert_eq!(initial["items"][0]["isPrimary"], true);
    let target = f
        .root
        .join("library")
        .join(initial["items"][0]["targetPath"].as_str().unwrap());
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"BBBB").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let (_, conflict) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(conflict["conflicts"], 1);
    if thumbnail {
        sqlx::query("UPDATE volund.import_drafts SET thumbnail_item_id=primary_item_id,primary_item_id=NULL").execute(&*f.db).await.unwrap();
    }
    let (resolved, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"skip"}),
    )
    .await;
    assert_eq!(resolved, StatusCode::BAD_REQUEST);
    // Old drafts may already contain the invalid resolution: review must remain blocked.
    sqlx::query("UPDATE volund.import_draft_items SET resolution='skip',planned_action='skip'")
        .execute(&*f.db)
        .await
        .unwrap();
    let (review_status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(review_status, StatusCode::OK);
    assert_eq!(review["conflicts"], 1);
    assert_eq!(review["skipFiles"], 0);
    let (status, _result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(target).unwrap(), b"BBBB");
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
