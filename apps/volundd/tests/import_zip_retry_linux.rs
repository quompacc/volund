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
async fn concurrent_zip_retries_publish_once_without_cross_cleanup() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let zip = tiny_zip();
    let (status, draft) = post(
        &f.router,
        "/api/v1/imports/preview",
        json!({"sourceName":"Archive","entries":[{"path":"tiny.zip","byteSize":zip.len()}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = draft["id"].as_str().unwrap();
    let item = draft["items"][0]["id"].as_str().unwrap();
    assert_eq!(post(&f.router, &format!("/api/v1/imports/{id}/metadata"), json!({"modelName":"Zip Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[]})).await.0, StatusCode::OK);
    let mut blocker = f.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,3)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let first = tokio::spawn(
        f.router
            .clone()
            .oneshot(upload_request(id, item, zip.clone())),
    );
    let first_ready = wait_for_publications(&f.db, 1).await;
    let second = tokio::spawn(
        f.router
            .clone()
            .oneshot(upload_request(id, item, zip.clone())),
    );
    let both_ready = wait_for_publications(&f.db, 2).await;
    let directory = f.root.join("incoming").join(id);
    let parts = std::fs::read_dir(&directory)
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".extract.part"))
        .count();
    blocker.rollback().await.unwrap();
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert!(
        first_ready && both_ready,
        "both attempts must finish extraction before publication"
    );
    assert_eq!(parts, 2);
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let third = f
        .router
        .clone()
        .oneshot(upload_request(id, item, zip.clone()))
        .await
        .unwrap();
    assert_eq!(third.status(), StatusCode::OK);
    let files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(files.len(), 2);
    assert_eq!(
        files
            .iter()
            .map(|e| e.metadata().unwrap().len())
            .sum::<u64>(),
        u64::try_from(zip.len() + 4).unwrap()
    );
    let rows:i64=sqlx::query_scalar("SELECT count(*) FROM volund.import_draft_items i JOIN volund.import_drafts d ON d.id=i.import_draft_id WHERE d.public_id::text=$1").bind(id).fetch_one(&*f.db).await.unwrap();
    assert_eq!(rows, 2);
    let (_, storage) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(storage["uploadedBytes"], zip.len() + 4);
    assert_eq!(storage["reservedBytes"], zip.len() + 4);
}
async fn wait_for_publications(db: &sqlx::PgPool, expected: i64) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let n:i64=sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE NOT granted AND locktype='advisory' AND classid=860756368 AND objid=3 AND objsubid=2 AND database=(SELECT oid FROM pg_database WHERE datname=current_database())").fetch_one(db).await.unwrap();
            if n == expected {break;}
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.is_ok()
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
