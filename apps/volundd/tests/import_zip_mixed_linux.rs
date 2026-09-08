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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn zip_keeps_incomplete_draft_resumable_until_remaining_upload_finishes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let zip = tiny_zip();
    let zip_size = zip.len();
    let (status,draft)=post(&f.router,"/api/v1/imports/preview",json!({"sourceName":"Mixed","entries":[{"path":"tiny.zip","byteSize":zip.len()},{"path":"notes.txt","byteSize":4}]})).await;
    assert_eq!(status, StatusCode::OK);
    let id = draft["id"].as_str().unwrap();
    let item = draft["items"][0]["id"].as_str().unwrap();
    let pending = draft["items"][1]["id"].as_str().unwrap();
    assert_eq!(post(&f.router,&format!("/api/v1/imports/{id}/metadata"),json!({"modelName":"Mixed Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[]})).await.0,StatusCode::OK);
    let response = f
        .router
        .clone()
        .oneshot(upload_request(id, item, zip))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (status, summary) = support::get_json(&f.router, &format!("/api/v1/imports/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(summary["status"], "uploading");
    assert_eq!(summary["uploadedFiles"], 2);
    assert_eq!(summary["totalFiles"], 3);
    assert_eq!(summary["uploadedBytes"], zip_size + 4);
    assert_eq!(summary["totalBytes"], zip_size + 8);
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert!(!status.is_success());
    let response = f
        .router
        .clone()
        .oneshot(upload_request(id, pending, b"note".to_vec()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (_, summary) = support::get_json(&f.router, &format!("/api/v1/imports/{id}")).await;
    assert_eq!(summary["status"], "uploaded");
    assert_eq!(summary["uploadedFiles"], 3);
    assert_eq!(summary["uploadedBytes"], zip_size + 8);
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    let state: String =
        sqlx::query_scalar("SELECT status FROM volund.import_drafts WHERE public_id::text=$1")
            .bind(id)
            .fetch_one(&*f.db)
            .await
            .unwrap();
    assert_eq!(state, "reviewed");
}
fn tiny_zip() -> Vec<u8> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "tiny.step",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(b"BBBB").unwrap();
    writer.finish().unwrap().into_inner()
}
fn upload_request(id: &str, item: &str, zip: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/imports/{id}/items/{item}/content"))
        .header("content-type", "application/octet-stream")
        .header("content-length", zip.len().to_string())
        .body(Body::from(zip))
        .unwrap()
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
