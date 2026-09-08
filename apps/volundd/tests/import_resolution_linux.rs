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
async fn review_preserves_and_revalidates_explicit_target() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload(false).await;
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let target = review["items"][0]["targetPath"].as_str().unwrap();
    let occupied = f.root.join("library").join(target);
    std::fs::create_dir_all(occupied.parent().unwrap()).unwrap();
    std::fs::write(&occupied, b"BBBB").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let (status, conflict) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conflict["conflicts"], 1);
    let item = conflict["items"][0]["id"].as_str().unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"create","targetPath":"safe/probe.step"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, refreshed) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refreshed["conflicts"], 0);
    assert_eq!(refreshed["items"][0]["targetPath"], "safe/probe.step");
    std::fs::create_dir_all(f.root.join("library/safe")).unwrap();
    std::fs::write(f.root.join("library/safe/probe.step"), b"BBBB").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let (_, changed) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(changed["conflicts"], 1);
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"create","targetPath":"safe/alternative.step"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, final_review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(final_review["conflicts"], 0);
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        std::fs::read(f.root.join("library/safe/alternative.step")).unwrap(),
        b"AAAA"
    );
    assert_eq!(std::fs::read(occupied).unwrap(), b"BBBB");
    assert_eq!(
        std::fs::read(f.root.join("library/safe/probe.step")).unwrap(),
        b"BBBB"
    );
}

#[tokio::test]
async fn skip_resolution_survives_repeated_review() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = f.upload(true).await;
    let (_, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    let item = review["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["isPrimary"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"skip"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for _ in 0..2 {
        let (status, review) = post(
            &f.router,
            &format!("/api/v1/imports/{id}/review"),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(review["skipFiles"], 1);
        assert_eq!(review["createFiles"], 1);
    }
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_files")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        1
    );
}
#[tokio::test]
async fn reuse_resolution_is_not_changed_back_to_relocation() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    std::fs::write(f.root.join("library/original.step"), b"AAAA").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id = f.upload(false).await;
    let (_, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(review["relocateFiles"], 1);
    let item = review["items"][0]["id"].as_str().unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"reuse"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(review["reuseFiles"], 1);
    assert_eq!(review["relocateFiles"], 0);
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        std::fs::read(f.root.join("library/original.step")).unwrap(),
        b"AAAA"
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
