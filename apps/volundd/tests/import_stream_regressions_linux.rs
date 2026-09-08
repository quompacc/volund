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
    async fn upload(&self) -> String {
        let (status, draft) = post(
            &self.router,
            "/api/v1/imports/preview",
            json!({"sourceName":"Probe","entries":[{"path":"Package/probe.step","byteSize":4}]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{draft}");
        let id = draft["id"].as_str().unwrap();
        let item = draft["items"][0]["id"].as_str().unwrap();
        let (status, body) = post(&self.router, &format!("/api/v1/imports/{id}/metadata"), json!({"modelName":"Review Probe","kind":"part","libraryRootId":self.library_id,"description":"","tags":[],"collectionIds":[]})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
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
        id.to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One file lifecycle checks full, conditional and range responses before and after reindexing.
async fn equal_length_change_rejects_stale_full_conditional_and_range_responses() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let path = f.root.join("library/probe.step");
    std::fs::write(&path, b"AAAA").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let uri = format!("/api/v1/files/{id}/content");
    let first = f
        .router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first.headers()["etag"].clone();
    std::fs::write(&path, b"BBBB").unwrap();
    let changed = f
        .router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(changed.headers().get("etag").is_none());
    let cached = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("if-none-match", etag.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cached.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let range = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("range", "bytes=1-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range.status(), StatusCode::INTERNAL_SERVER_ERROR);
    volundd::scanner::scan_root(&f.db, "probe", true)
        .await
        .unwrap();
    let fresh = f
        .router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::OK);
    let fresh_etag = fresh.headers()["etag"].clone();
    assert_ne!(fresh_etag, etag);
    assert_eq!(
        to_bytes(fresh.into_body(), 1024).await.unwrap().as_ref(),
        b"BBBB"
    );
    let unchanged = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("if-none-match", fresh_etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    let range = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("range", "bytes=1-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        to_bytes(range.into_body(), 1024).await.unwrap().as_ref(),
        b"BB"
    );
}

#[tokio::test]
async fn identical_primary_at_library_root_reviews_and_commits() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    std::fs::write(f.root.join("library/probe.step"), b"AAAA").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id = f.upload().await;
    let (status, body) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["baseDirectory"], "review-probe");
    let target = f
        .root
        .join("library")
        .join(body["items"][0]["targetPath"].as_str().unwrap());
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(std::fs::read(target).unwrap(), b"AAAA");
}

#[tokio::test]
async fn expired_reviewed_import_cannot_publish_files() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload().await;
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    sqlx::query("UPDATE volund.import_drafts SET expires_at=now()-interval '1 second' WHERE public_id::text=$1").bind(&id).execute(&*f.db).await.unwrap();
    let (review_status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(review_status, StatusCode::NOT_FOUND);
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{result}");
    assert!(
        !f.root
            .join("library")
            .join(review["items"][0]["targetPath"].as_str().unwrap())
            .exists()
    );
    assert!(f.root.join("incoming").join(&id).exists());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.models")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM volund.import_drafts")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        "reviewed"
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
    (status, serde_json::from_slice(&body).unwrap())
}
