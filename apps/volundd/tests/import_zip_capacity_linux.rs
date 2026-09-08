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
async fn zip_capacity_counts_cleanup_and_shares_preview_lock() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    retain_committed_upload(&f).await;
    let zip = tiny_zip();
    let zip_size = i64::try_from(zip.len()).unwrap();
    let (id, item) = prepare_archive(&f, zip_size).await;
    let (_, before) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(
        before["reservedBytes"].as_i64().unwrap() + before["reclaimableBytes"].as_i64().unwrap(),
        20_i64 * 1024 * 1024 * 1024
    );
    let mut blocker = f.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,3)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let request = upload_request(&id, &item, zip.clone());
    let upload = tokio::spawn(f.router.clone().oneshot(request));
    let preview_router = f.router.clone();
    let preview = tokio::spawn(async move {
        post(
            &preview_router,
            "/api/v1/imports/preview",
            json!({"sourceName":"Concurrent","entries":[{"path":"other.step","byteSize":4}]}),
        )
        .await
        .0
    });
    let waiting = wait_for_capacity_locks(&f.db).await;
    blocker.rollback().await.unwrap();
    let response = upload.await.unwrap().unwrap();
    assert_eq!(preview.await.unwrap(), StatusCode::BAD_REQUEST);
    assert!(
        waiting,
        "both ZIP and preview must wait on the shared capacity lock"
    );
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("zip_incoming_capacity_exceeded"));
    let (_, rejected) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(rejected["reservedBytes"], before["reservedBytes"]);
    assert_eq!(rejected["reclaimableBytes"], 4);
    assert_archive_files(&f, &id, 1, u64::try_from(zip_size).unwrap()).await;
    volundd::import_cleanup::run(&f.db, &f.root.join("incoming"))
        .await
        .unwrap();
    let retry = f
        .router
        .clone()
        .oneshot(upload_request(&id, &item, zip.clone()))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    assert_archive_files(&f, &id, 2, u64::try_from(zip_size + 4).unwrap()).await;
    let (_, storage) = support::get_json(&f.router, "/api/v1/imports/storage").await;
    assert_eq!(storage["reservedBytes"], storage["capacityBytes"]);
    let retry = f
        .router
        .clone()
        .oneshot(upload_request(&id, &item, zip))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    assert_archive_files(&f, &id, 2, u64::try_from(zip_size + 4).unwrap()).await;
}
async fn retain_committed_upload(f: &Fixture) {
    use std::os::unix::fs::PermissionsExt;
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
    let staged = f.root.join("incoming").join(&id);
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o555)).unwrap();
    let commit = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!commit.0.is_success());
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
async fn prepare_archive(f: &Fixture, zip_size: i64) -> (String, String) {
    for n in 0..2 {
        let entries: Vec<Value> = (0..5).map(|i| json!({"path":format!("reserve-{i}.step"),"byteSize":2_i64*1024*1024*1024-if n==1 && i==0 {zip_size+4} else {0}})).collect();
        assert_eq!(
            post(
                &f.router,
                "/api/v1/imports/preview",
                json!({"sourceName":"Reservation","entries":entries})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (status, draft) = post(
        &f.router,
        "/api/v1/imports/preview",
        json!({"sourceName":"Archive","entries":[{"path":"tiny.zip","byteSize":zip_size}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = draft["id"].as_str().unwrap();
    let item = draft["items"][0]["id"].as_str().unwrap();
    assert_eq!(post(&f.router, &format!("/api/v1/imports/{id}/metadata"), json!({"modelName":"Zip Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[]})).await.0, StatusCode::OK);
    (id.to_owned(), item.to_owned())
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
async fn wait_for_capacity_locks(db: &sqlx::PgPool) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE NOT granted AND locktype='advisory' AND classid=860756368 AND objid=3 AND objsubid=2 AND database=(SELECT oid FROM pg_database WHERE datname=current_database())").fetch_one(db).await.unwrap();
            if waiting == 2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.is_ok()
}
async fn assert_archive_files(f: &Fixture, id: &str, count: i64, bytes: u64) {
    let files: Vec<_> = std::fs::read_dir(f.root.join("incoming").join(id))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(i64::try_from(files.len()).unwrap(), count);
    assert_eq!(
        files
            .iter()
            .map(|e| e.metadata().unwrap().len())
            .sum::<u64>(),
        bytes
    );
    let rows:i64=sqlx::query_scalar("SELECT count(*) FROM volund.import_draft_items i JOIN volund.import_drafts d ON d.id=i.import_draft_id WHERE d.public_id::text=$1").bind(id).fetch_one(&*f.db).await.unwrap();
    assert_eq!(rows, count);
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
