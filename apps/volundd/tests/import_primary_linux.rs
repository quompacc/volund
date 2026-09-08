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
async fn updating_import_retains_relocated_primary() {
    update_primary(false).await;
}

#[tokio::test]
async fn updating_import_retains_reused_primary() {
    update_primary(true).await;
}

async fn update_primary(reuse: bool) {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let first = f.upload(false).await;
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{first}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, created) = post(
        &f.router,
        &format!("/api/v1/imports/{first}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM volund.model_source_files WHERE is_primary"
        )
        .fetch_one(&*f.db)
        .await
        .unwrap(),
        1
    );
    let original_source: i64 =
        sqlx::query_scalar("SELECT source_file_id FROM volund.model_source_files WHERE is_primary")
            .fetch_one(&*f.db)
            .await
            .unwrap();
    let original_path: String =
        sqlx::query_scalar("SELECT relative_path FROM volund.source_files WHERE id=$1")
            .bind(original_source)
            .fetch_one(&*f.db)
            .await
            .unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM volund.models")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let second = f.upload(false).await;
    let item:String=sqlx::query_scalar("SELECT i.public_id::text FROM volund.import_draft_items i JOIN volund.import_drafts d ON d.id=i.import_draft_id WHERE d.public_id::text=$1").bind(&second).fetch_one(&*f.db).await.unwrap();
    let (status,body)=post(&f.router,&format!("/api/v1/imports/{second}/metadata"),json!({"modelName":"Review Probe","kind":"part","libraryRootId":f.library_id,"description":"","tags":[],"collectionIds":[],"targetAction":"update","targetModelId":created["modelId"],"expectedModelRevision":revision,"primaryItemId":item})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{second}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["items"][0]["isPrimary"], true);
    if reuse {
        let (status, _) = post(
            &f.router,
            &format!("/api/v1/imports/{second}/items/{item}/resolution"),
            json!({"action":"reuse","targetPath":original_path}),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{second}/commit"),
        json!({}),
    )
    .await;
    let primary: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.model_source_files WHERE is_primary")
            .fetch_one(&*f.db)
            .await
            .unwrap();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(primary, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT source_file_id FROM volund.model_source_files WHERE is_primary"
        )
        .fetch_one(&*f.db)
        .await
        .unwrap(),
        original_source
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.model_source_files")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        1
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
