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
async fn duplicate_content_uses_one_relocation_and_one_new_source() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    std::fs::write(f.root.join("library/probe.step"), b"AAAA").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let original_id: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let id = f.upload(true).await;
    let (status, review) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["relocateFiles"], 1);
    assert_eq!(review["createFiles"], 1);
    assert_eq!(review["newBytes"], 4);
    // A persistence failure must leave the original and remove BOTH staged destinations.
    sqlx::query("CREATE FUNCTION volund.fail_duplicate_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test import audit failure'; END $$").execute(&*f.db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_duplicate_probe BEFORE INSERT ON volund.security_audit_events FOR EACH ROW WHEN (NEW.action='model.import.commit') EXECUTE FUNCTION volund.fail_duplicate_probe()").execute(&*f.db).await.unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert!(!status.is_success());
    sqlx::query("DROP TRIGGER fail_duplicate_probe ON volund.security_audit_events")
        .execute(&*f.db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.fail_duplicate_probe()")
        .execute(&*f.db)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(f.root.join("library/probe.step")).unwrap(),
        b"AAAA"
    );
    for item in review["items"].as_array().unwrap() {
        assert!(
            !f.root
                .join("library")
                .join(item["targetPath"].as_str().unwrap())
                .exists()
        );
    }
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    for item in review["items"].as_array().unwrap() {
        assert_eq!(
            std::fs::read(
                f.root
                    .join("library")
                    .join(item["targetPath"].as_str().unwrap())
            )
            .unwrap(),
            b"AAAA"
        );
    }
    assert!(!f.root.join("library/probe.step").exists());
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM volund.source_files WHERE missing_at IS NULL"
        )
        .fetch_one(&*f.db)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM volund.source_files WHERE public_id::text=$1"
        )
        .bind(original_id)
        .fetch_one(&*f.db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn stale_duplicate_relocation_plan_is_rejected_before_files_change() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    std::fs::write(f.root.join("library/probe.step"), b"AAAA").unwrap();
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    let id = f.upload(true).await;
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/review"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE volund.import_draft_items SET planned_action='relocate',matched_source_file_id=(SELECT id FROM volund.source_files LIMIT 1)").execute(&*f.db).await.unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read(f.root.join("library/probe.step")).unwrap(),
        b"AAAA"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.models")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn quarantine_import_targets_are_rejected_at_resolution_and_commit() {
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
    let item = review["items"][0]["id"].as_str().unwrap();
    for target in [
        ".volund-quarantine/probe.step",
        ".volund-quarantine/nested/probe.step",
    ] {
        let (status, _) = post(
            &f.router,
            &format!("/api/v1/imports/{id}/items/{item}/resolution"),
            json!({"action":"create","targetPath":target}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    // Simulate a plan stored by an older version: commit must independently refuse it.
    sqlx::query("UPDATE volund.import_draft_items SET planned_relative_path='.volund-quarantine/probe.step'").execute(&*f.db).await.unwrap();
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!f.root.join("library/.volund-quarantine").exists());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.models")
            .fetch_one(&*f.db)
            .await
            .unwrap(),
        0
    );
    let (status, _) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/items/{item}/resolution"),
        json!({"action":"create","targetPath":".volund-quarantine-other/probe.step"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, result) = post(
        &f.router,
        &format!("/api/v1/imports/{id}/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    volundd::scanner::scan_root(&f.db, "probe", false)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM volund.source_files WHERE missing_at IS NULL"
        )
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
