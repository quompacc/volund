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

// Exercise cancellation after filesystem staging but before the catalog commit.

#[tokio::test]
async fn interrupted_import_preserves_catalog_source_and_can_retry() {
    interrupted_import_can_retry(true).await;
}

#[tokio::test]
async fn interrupted_new_import_removes_staged_files_and_can_retry() {
    interrupted_import_can_retry(false).await;
}

#[allow(clippy::too_many_lines)] // Keep the interrupted transaction and its retry in one scenario.
async fn interrupted_import_can_retry(relocate: bool) {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let original = f.root.join("library/probe.step");
    if relocate {
        std::fs::write(&original, b"AAAA").unwrap();
    }
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id = f.upload().await;
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(
        review["items"][0]["action"],
        if relocate { "relocate" } else { "create" }
    );
    let target = f
        .root
        .join("library")
        .join(review["items"][0]["targetPath"].as_str().unwrap());
    let mut barrier = f.db.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(674839)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION volund.pause_import_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(674839); RETURN NEW; END $$").execute(&*f.db).await.unwrap();
    sqlx::query("CREATE TRIGGER pause_import_probe BEFORE INSERT ON volund.security_audit_events FOR EACH ROW WHEN (NEW.action='model.import.commit') EXECUTE FUNCTION volund.pause_import_probe()").execute(&*f.db).await.unwrap();
    let router = f.router.clone();
    let endpoint = format!("/api/v1/imports/{id}/commit");
    let task = tokio::spawn(async move { post(&router, &endpoint, json!({})).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objid=674839 AND NOT granted)").fetch_one(&*f.db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    sqlx::query("SELECT pg_advisory_unlock(674839)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    drop(barrier);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sqlx::query("DROP TRIGGER pause_import_probe ON volund.security_audit_events")
            .execute(&*f.db),
    )
    .await
    .unwrap()
    .unwrap();
    sqlx::query("DROP FUNCTION volund.pause_import_probe()")
        .execute(&*f.db)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM volund.import_drafts")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        "reviewed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT relative_path FROM volund.source_files")
            .fetch_optional(&*f.db)
            .await
            .unwrap(),
        relocate.then(|| "probe.step".to_owned())
    );
    if relocate {
        assert_eq!(std::fs::read(&original).unwrap(), b"AAAA");
    }
    assert!(!target.exists());
    assert!(
        std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".part")
            })
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.models")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(!original.exists());
    assert_eq!(std::fs::read(target).unwrap(), b"AAAA");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn moving_through_symlink_directory_is_rejected_without_changing_identity() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let library = f.root.join("library");
    std::fs::write(library.join("probe.step"), b"AAAA").unwrap();
    std::fs::create_dir(library.join("real")).unwrap();
    std::os::unix::fs::symlink(library.join("real"), library.join("alias")).unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let (status, moved) = post(
        &f.router,
        &format!("/api/v1/files/{id}/move"),
        json!({"destinationDirectory":"alias"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{moved}");
    assert!(!library.join("real/probe.step").exists());
    assert_eq!(std::fs::read(library.join("probe.step")).unwrap(), b"AAAA");
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let missing: bool = sqlx::query_scalar(
        "SELECT missing_at IS NOT NULL FROM volund.source_files WHERE public_id::text=$1",
    )
    .bind(&id)
    .fetch_one(&*f.db)
    .await
    .unwrap();
    assert!(!missing);
    let replacement: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files WHERE relative_path='probe.step' AND missing_at IS NULL").fetch_one(&*f.db).await.unwrap();
    assert_eq!(replacement, id);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_files")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        1
    );
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/files/{id}/move"),
        json!({"destinationDirectory":"real"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let moved_id: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files WHERE relative_path='real/probe.step' AND missing_at IS NULL").fetch_one(&*f.db).await.unwrap();
    assert_eq!(moved_id, id);
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
