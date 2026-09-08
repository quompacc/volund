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
async fn rejected_zip_blocks_review_and_legacy_commit() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let d = configured(
        &f,
        json!([{"path":"bad.zip","byteSize":4},{"path":"part.step","byteSize":4}]),
    )
    .await;
    let id = d["id"].as_str().unwrap();
    for path in ["bad.zip", "part.step"] {
        let item = d["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["originalPath"] == path)
            .unwrap_or_else(|| panic!("manifest {d}"));
        let r = f
            .router
            .clone()
            .oneshot(upload_request(
                id,
                item["id"].as_str().unwrap(),
                b"BBBB".to_vec(),
            ))
            .await
            .unwrap();
        let status = r.status();
        let b = to_bytes(r.into_body(), 1_048_576).await.unwrap();
        println!("upload {path}: {status} {}", String::from_utf8_lossy(&b));
    }
    let review = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(review.0, StatusCode::BAD_REQUEST);
    // Simulate a persisted plan from the previous version, which accepted raw ZIPs.
    sqlx::query(
        "UPDATE volund.import_draft_items SET category='cad' WHERE original_path='bad.zip'",
    )
    .execute(&*f.db)
    .await
    .unwrap();
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
    sqlx::query(
        "UPDATE volund.import_draft_items SET category='archive' WHERE original_path='bad.zip'",
    )
    .execute(&*f.db)
    .await
    .unwrap();
    let commit = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    println!("commit: {commit:?}");
    assert_eq!(commit.0, StatusCode::BAD_REQUEST);
    assert!(!f.root.join("library/Bauteile/batch-probe").exists());
}
#[tokio::test]
async fn aborted_ordinary_upload_cleans_up_and_allows_retry() {
    use futures_util::StreamExt;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let d = configured(&f, json!([{"path":"part.step","byteSize":8}])).await;
    let id = d["id"].as_str().unwrap();
    let item = d["items"][0]["id"].as_str().unwrap();
    let stream = futures_util::stream::once(async {
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"AAAA"))
    })
    .chain(futures_util::stream::pending());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/imports/{id}/items/{item}/content"))
        .header("content-length", "8")
        .body(Body::from_stream(stream))
        .unwrap();
    let task = tokio::spawn(f.router.clone().oneshot(req));
    let part = f
        .root
        .join("incoming")
        .join(id)
        .join(format!("{item}.part"));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if std::fs::metadata(&part).is_ok_and(|m| m.len() == 4) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let status: String = sqlx::query_scalar(
                "SELECT upload_status FROM volund.import_draft_items WHERE public_id::text=$1",
            )
            .bind(item)
            .fetch_one(&*f.db)
            .await
            .unwrap();
            if status == "pending" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(!part.exists());
    let r = f
        .router
        .clone()
        .oneshot(upload_request(id, item, b"AAAABBBB".to_vec()))
        .await
        .unwrap();
    let status = r.status();
    let b = to_bytes(r.into_body(), 1_048_576).await.unwrap();
    println!("retry: {status} {}", String::from_utf8_lossy(&b));
    assert_eq!(status, StatusCode::OK);
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
