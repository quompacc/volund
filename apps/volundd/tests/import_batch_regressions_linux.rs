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
async fn resume_manifest_includes_server_extracted_files() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let zip = tiny_zip();
    let draft = configured(
        &f,
        json!([{"path":"tiny.zip","byteSize":zip.len()},{"path":"notes.txt","byteSize":4}]),
    )
    .await;
    let id = draft["id"].as_str().unwrap();
    let item = draft["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        f.router
            .clone()
            .oneshot(upload_request(id, item, zip))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let (status, manifest) =
        support::get_json(&f.router, &format!("/api/v1/imports/{id}/manifest")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(manifest["items"].as_array().unwrap().len(), 3);
    assert!(
        manifest["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["originalPath"] == "tiny.step")
    );
}
#[tokio::test]
async fn expiry_during_zip_extraction_prevents_publication() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let zip = tiny_zip();
    let draft = configured(&f, json!([{"path":"tiny.zip","byteSize":zip.len()}])).await;
    let id = draft["id"].as_str().unwrap();
    let item = draft["items"][0]["id"].as_str().unwrap();
    let mut lock = f.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(860756368,3)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let request = tokio::spawn(f.router.clone().oneshot(upload_request(id, item, zip)));
    let ready = wait_for_publications(&f.db, 1).await;
    sqlx::query("UPDATE volund.import_drafts SET expires_at=clock_timestamp()+interval '100 milliseconds' WHERE public_id::text=$1").bind(id).execute(&*f.db).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    lock.rollback().await.unwrap();
    let response = request.await.unwrap().unwrap();
    assert!(ready);
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let expanded:bool=sqlx::query_scalar("SELECT archive_expanded_at IS NOT NULL FROM volund.import_draft_items WHERE public_id::text=$1").bind(item).fetch_one(&*f.db).await.unwrap();
    let (review_status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert!(!expanded);
    assert_eq!(
        std::fs::read_dir(f.root.join("incoming").join(id))
            .unwrap()
            .count(),
        1
    );
    assert!(!review_status.is_success());
}
#[tokio::test]
async fn concurrent_upload_completion_preserves_progress() {
    use axum::body::Bytes;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let draft = configured(
        &f,
        json!([{"path":"one.step","byteSize":4},{"path":"two.step","byteSize":4}]),
    )
    .await;
    let id = draft["id"].as_str().unwrap();
    let mut senders = Vec::new();
    let mut tasks = Vec::new();
    for item in draft["items"].as_array().unwrap() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        senders.push(tx);
        let body = Body::from_stream(futures_util::stream::once(async move {
            rx.await.unwrap();
            Ok::<_, std::io::Error>(Bytes::from_static(b"BBBB"))
        }));
        let req = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/imports/{id}/items/{}/content",
                item["id"].as_str().unwrap()
            ))
            .header("content-type", "application/octet-stream")
            .header("content-length", "4")
            .body(body)
            .unwrap();
        tasks.push(tokio::spawn(f.router.clone().oneshot(req)));
    }
    let acquired = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM volund.import_draft_items WHERE upload_status='uploading'",
            )
            .fetch_one(&*f.db)
            .await
            .unwrap();
            if n == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    let mut lock = f.db.begin().await.unwrap();
    sqlx::query("SELECT id FROM volund.import_drafts WHERE public_id::text=$1 FOR UPDATE")
        .bind(id)
        .execute(&mut *lock)
        .await
        .unwrap();
    for tx in senders {
        tx.send(()).unwrap();
    }
    let blocked=tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let n:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND (query LIKE 'UPDATE volund.import_drafts d SET uploaded_bytes=items.bytes%' OR query LIKE 'SELECT d.id FROM volund.import_drafts d WHERE d.id=%')").fetch_one(&*f.db).await.unwrap();
            if n==2 {break;}
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await;
    lock.rollback().await.unwrap();
    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap().status(), StatusCode::OK);
    }
    assert!(acquired.is_ok() && blocked.is_ok());
    let (_, summary) = support::get_json(&f.router, &format!("/api/v1/imports/{id}")).await;
    assert_eq!(summary["uploadedFiles"], 2);
    assert_eq!(summary["uploadedBytes"], 8);
    assert_eq!(summary["status"], "uploaded");
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
}
async fn configured(f: &Fixture, entries: Value) -> Value {
    let (status, draft) = post(
        &f.router,
        "/api/v1/imports/preview",
        json!({"sourceName":"Batch","entries":entries}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = draft["id"].as_str().unwrap();
    assert_eq!(post(&f.router,&format!("/api/v1/imports/{id}/metadata"),json!({"modelName":"Batch Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[]})).await.0,StatusCode::OK);
    draft
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
