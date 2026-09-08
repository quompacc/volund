#![cfg(target_os = "linux")]
mod support;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    db: support::TestDatabase,
    router: axum::Router,
    root: std::path::PathBuf,
    source: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = support::test_database().await?;
        let root = std::env::temp_dir().join(format!(
            "volund-source-failure-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("probe.stl"), b"solid failure fixture").unwrap();
        volundd::scanner::register_root(&db, "failure", "Failure fixture", &root)
            .await
            .unwrap();
        volundd::scanner::scan_root(&db, "failure", false)
            .await
            .unwrap();
        let source = sqlx::query_scalar(
            "SELECT public_id::text FROM volund.source_files WHERE relative_path='probe.stl'",
        )
        .fetch_one(&*db)
        .await
        .unwrap();
        let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
        Some(Self {
            db,
            router,
            root,
            source,
        })
    }

    async fn plan(&self, action: &str, revision: i64) -> Value {
        let (status, plan) = post(
            &self.router,
            "/api/v1/lifecycle/preview",
            json!({"action":action,"targetId":self.source,"expectedRevision":revision}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        plan
    }

    async fn apply(&self, plan: &Value) -> StatusCode {
        post(
            &self.router,
            &format!(
                "/api/v1/lifecycle/plans/{}/apply",
                plan["id"].as_str().unwrap()
            ),
            json!({"confirmation":plan["confirmation"]}),
        )
        .await
        .0
    }

    fn quarantined(&self) -> std::path::PathBuf {
        self.root
            .join(".volund-quarantine")
            .join(format!("{}-1", self.source))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn audit_and_commit_failures_preserve_bytes_and_allow_retry() {
    for deferred in [false, true] {
        for action in ["source.quarantine", "source.recover", "source.purge"] {
            let Some(f) = Fixture::new().await else {
                return;
            };
            if action != "source.quarantine" {
                assert_eq!(
                    f.apply(&f.plan("source.quarantine", 1).await).await,
                    StatusCode::OK
                );
            }
            if action == "source.purge" {
                sqlx::query("UPDATE volund.source_quarantines SET retention_until=now()-interval '1 second'").execute(&*f.db).await.unwrap();
            }
            let plan = f.plan(action, 1).await;
            sqlx::query(&format!("CREATE FUNCTION volund.reject_lifecycle_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='{action}' THEN RAISE EXCEPTION 'injected audit failure'; END IF; RETURN NEW; END $$"))
                .execute(&*f.db).await.unwrap();
            let trigger = if deferred {
                "CREATE CONSTRAINT TRIGGER reject_lifecycle_test AFTER INSERT ON volund.security_audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION volund.reject_lifecycle_test()"
            } else {
                "CREATE TRIGGER reject_lifecycle_test BEFORE INSERT ON volund.security_audit_events FOR EACH ROW EXECUTE FUNCTION volund.reject_lifecycle_test()"
            };
            sqlx::query(trigger).execute(&*f.db).await.unwrap();
            let status = f.apply(&plan).await;
            sqlx::query("DROP TRIGGER reject_lifecycle_test ON volund.security_audit_events")
                .execute(&*f.db)
                .await
                .unwrap();
            sqlx::query("DROP FUNCTION volund.reject_lifecycle_test()")
                .execute(&*f.db)
                .await
                .unwrap();
            assert_eq!(
                status,
                StatusCode::INTERNAL_SERVER_ERROR,
                "{action}, deferred={deferred}"
            );
            let owned = if action == "source.quarantine" {
                f.root.join("probe.stl")
            } else {
                f.quarantined()
            };
            assert_eq!(std::fs::read(owned).unwrap(), b"solid failure fixture");
            let state: String =
                sqlx::query_scalar("SELECT lifecycle_state FROM volund.source_files")
                    .fetch_one(&*f.db)
                    .await
                    .unwrap();
            assert_eq!(
                state,
                if action == "source.quarantine" {
                    "available"
                } else {
                    "quarantined"
                }
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
                    .fetch_one(&*f.db)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(f.apply(&plan).await, StatusCode::OK, "retry {action}");
        }
    }
}

#[tokio::test]
async fn cleanup_resumes_after_commit_without_deleting_the_last_or_replaced_copy() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    assert_eq!(
        f.apply(&f.plan("source.quarantine", 1).await).await,
        StatusCode::OK
    );
    let keeper = f.root.join("probe.stl");
    // Simulate a committed recovery interrupted before its final filesystem cleanup.
    sqlx::query("INSERT INTO volund.source_cleanup (source_file_id,obsolete_relative_path,keeper_relative_path,sha256,byte_size) SELECT source_file_id,quarantine_relative_path,original_relative_path,sha256,byte_size FROM volund.source_quarantines").execute(&*f.db).await.unwrap();
    sqlx::query("UPDATE volund.source_files SET lifecycle_state='available'")
        .execute(&*f.db)
        .await
        .unwrap();
    sqlx::query("UPDATE volund.source_quarantines SET state='recovered'")
        .execute(&*f.db)
        .await
        .unwrap();
    assert!(volundd::source_cleanup::reconcile(&f.db).await.is_err());
    assert!(f.quarantined().exists());
    std::fs::write(&keeper, b"solid failure fixture").unwrap();
    assert!(
        volundd::source_cleanup::reconcile(&f.db).await.is_err(),
        "equal bytes are not permission to overwrite another inode"
    );
    std::fs::remove_file(&keeper).unwrap();
    std::fs::hard_link(f.quarantined(), &keeper).unwrap();
    std::fs::write(&keeper, b"solid altered fixture").unwrap();
    assert!(
        volundd::source_cleanup::reconcile(&f.db).await.is_err(),
        "changed bytes must survive cleanup"
    );
    assert!(f.quarantined().exists());
    std::fs::write(&keeper, b"solid failure fixture").unwrap();
    sqlx::query("CREATE FUNCTION volund.reject_cleanup_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected cleanup persistence failure'; END $$").execute(&*f.db).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_cleanup_test BEFORE DELETE ON volund.source_cleanup FOR EACH ROW EXECUTE FUNCTION volund.reject_cleanup_test()").execute(&*f.db).await.unwrap();
    assert!(volundd::source_cleanup::reconcile(&f.db).await.is_err());
    sqlx::query("DROP TRIGGER reject_cleanup_test ON volund.source_cleanup")
        .execute(&*f.db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.reject_cleanup_test()")
        .execute(&*f.db)
        .await
        .unwrap();
    assert!(!f.quarantined().exists());
    assert_eq!(std::fs::read(&keeper).unwrap(), b"solid failure fixture");
    assert_eq!(volundd::source_cleanup::reconcile(&f.db).await.unwrap(), 1);
    assert_eq!(volundd::source_cleanup::reconcile(&f.db).await.unwrap(), 0);
}

#[tokio::test]
async fn recovery_refuses_an_existing_foreign_file_even_with_equal_bytes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    assert_eq!(
        f.apply(&f.plan("source.quarantine", 1).await).await,
        StatusCode::OK
    );
    let original = f.root.join("probe.stl");
    std::fs::write(&original, b"solid failure fixture").unwrap();
    assert_eq!(
        f.apply(&f.plan("source.recover", 1).await).await,
        StatusCode::CONFLICT
    );
    assert_eq!(std::fs::read(original).unwrap(), b"solid failure fixture");
    assert_eq!(
        std::fs::read(f.quarantined()).unwrap(),
        b"solid failure fixture"
    );
}

async fn post(router: &axum::Router, uri: &str, value: Value) -> (StatusCode, Value) {
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
