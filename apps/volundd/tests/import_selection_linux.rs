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
                        .body(Body::from(
                            if entry["originalPath"]
                                .as_str()
                                .unwrap()
                                .ends_with("copy.step")
                            {
                                "BBBB"
                            } else {
                                "AAAA"
                            },
                        ))
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
async fn review_and_commit_use_selected_primary() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    std::fs::create_dir_all(f.root.join("library/selected")).unwrap();
    std::fs::create_dir_all(f.root.join("library/suggested")).unwrap();
    std::fs::write(f.root.join("library/selected/probe.step"), b"AAAA").unwrap();
    std::fs::write(f.root.join("library/suggested/copy.step"), b"BBBB").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id = f.upload(true).await;
    let (status, first) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let selected = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["isPrimary"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status,body)=post(&f.router,&format!("/api/v1/imports/{id}/metadata"),json!({"modelName":"Review Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[],"primaryItemId":selected})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let marked = review["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["isPrimary"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let actual_item = review["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == selected)
        .unwrap();
    assert_eq!(selected, marked);
    assert_eq!(review["baseDirectory"], "selected/review-probe");
    let (status, committed) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    let actual_path:String=sqlx::query_scalar("SELECT s.relative_path FROM volund.source_files s JOIN volund.model_source_files l ON l.source_file_id=s.id WHERE l.is_primary").fetch_one(&*f.db).await.unwrap();
    assert_eq!(actual_item["targetPath"], actual_path);
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
