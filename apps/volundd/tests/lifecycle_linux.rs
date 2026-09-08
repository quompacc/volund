#![cfg(target_os = "linux")]

mod support;

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;
use volundd::api;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn lifecycle_plans_unlink_quarantine_recover_and_purge_without_overwrite() {
    let Some(database) = support::test_database().await else {
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-lifecycle-{unique}"));
    let derived = root.join("derived");
    let library = root.join("library");
    let web = root.join("web");
    fs::create_dir_all(&derived).expect("create derived root");
    fs::create_dir_all(&library).expect("create library root");
    fs::create_dir_all(&web).expect("create web root");
    fs::write(web.join("index.html"), "ok").expect("write web fixture");
    fs::write(library.join("part.stl"), b"solid lifecycle fixture").expect("write source");
    let digest = volundd::file_hash::sha256_file(&library.join("part.stl")).expect("hash source");
    let byte_size = i64::try_from(fs::metadata(library.join("part.stl")).unwrap().len()).unwrap();

    let fixture = sqlx::query(
        "WITH root AS (
           INSERT INTO volund.library_roots (root_key,display_name,filesystem_path)
           VALUES ('life','Lifecycle',$1) RETURNING id
         ), scan AS (
           INSERT INTO volund.scan_runs (library_root_id,status)
           SELECT id,'completed' FROM root RETURNING id,library_root_id
         ), content AS (
           INSERT INTO volund.content_objects (sha256,byte_size,detected_format)
           VALUES ($2,$3,'stl') RETURNING id
         ), source AS (
           INSERT INTO volund.source_files
             (library_root_id,content_object_id,relative_path,filesystem_modified_at,last_seen_scan_id)
           SELECT scan.library_root_id,content.id,'part.stl',now(),scan.id FROM scan,content
           RETURNING id,public_id
         ), model AS (
           INSERT INTO volund.models (slug,name,kind) VALUES ('lifecycle-model','Lifecycle Model','part')
           RETURNING id,public_id
         )
         INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary)
         SELECT model.id,source.id,'printable-mesh',true FROM model,source
         RETURNING (SELECT public_id::text FROM model),(SELECT public_id::text FROM source)",
    )
    .bind(library.to_string_lossy().as_ref())
    .bind(digest.as_str())
    .bind(byte_size)
    .fetch_one(&*database)
    .await
    .expect("create lifecycle fixture");
    let model_id: String = fixture.get(0);
    let source_id: String = fixture.get(1);
    let router = support::authenticated_router(
        &database,
        api::router_with_roots(database.clone(), derived, web),
    )
    .await;

    let unlink = preview(&router, "model-file.unlink", &source_id, Some(&model_id), 1).await;
    let (wrong, _) = post(
        &router,
        &format!(
            "/api/v1/lifecycle/plans/{}/apply",
            unlink["id"].as_str().unwrap()
        ),
        json!({"confirmation":"wrong"}),
    )
    .await;
    assert_eq!(wrong, StatusCode::BAD_REQUEST);
    let (status, result) = post(
        &router,
        &format!(
            "/api/v1/lifecycle/plans/{}/apply",
            unlink["id"].as_str().unwrap()
        ),
        json!({"confirmation":unlink["confirmation"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["revision"], 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.model_source_files")
            .fetch_one(&*database)
            .await
            .unwrap(),
        0
    );

    let quarantine = preview(&router, "source.quarantine", &source_id, None, 1).await;
    assert_eq!(quarantine["impact"]["liveModelLinks"], 0);
    apply(&router, &quarantine).await;
    assert!(!library.join("part.stl").exists());
    assert!(
        library
            .join(".volund-quarantine")
            .join(format!("{source_id}-1"))
            .exists()
    );

    let recovery = preview(&router, "source.recover", &source_id, None, 1).await;
    apply(&router, &recovery).await;
    assert_eq!(
        fs::read(library.join("part.stl")).unwrap(),
        b"solid lifecycle fixture"
    );

    let quarantine_again = preview(&router, "source.quarantine", &source_id, None, 3).await;
    apply(&router, &quarantine_again).await;
    let (inventory_status, inventory) =
        support::get_json(&router, "/api/v1/quarantines?limit=100&offset=0").await;
    assert_eq!(inventory_status, StatusCode::OK);
    assert_eq!(inventory["items"][0]["sourceId"], source_id);
    assert!(inventory["items"][0].get("filesystemPath").is_none());
    sqlx::query("UPDATE volund.source_quarantines SET retention_until=now()-interval '1 second' WHERE state='quarantined'")
        .execute(&*database).await.expect("expire quarantine fixture");
    let purge = preview(&router, "source.purge", &source_id, None, 1).await;
    assert_eq!(purge["impact"]["retentionExpired"], true);
    apply(&router, &purge).await;
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT lifecycle_state FROM volund.source_files WHERE public_id::text=$1"
        )
        .bind(&source_id)
        .fetch_one(&*database)
        .await
        .unwrap(),
        "purged"
    );
    assert!(
        !library
            .join(".volund-quarantine")
            .join(format!("{source_id}-3"))
            .exists()
    );

    fs::remove_dir_all(root).expect("remove lifecycle fixture");
}

async fn preview(
    router: &Router,
    action: &str,
    target: &str,
    parent: Option<&str>,
    revision: i64,
) -> Value {
    let (status, body) = post(
        router,
        "/api/v1/lifecycle/preview",
        json!({"action":action,"targetId":target,"parentId":parent,"expectedRevision":revision}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview failed: {body}");
    body
}

async fn apply(router: &Router, plan: &Value) {
    let (status, body) = post(
        router,
        &format!(
            "/api/v1/lifecycle/plans/{}/apply",
            plan["id"].as_str().unwrap()
        ),
        json!({"confirmation":plan["confirmation"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "apply failed: {body}");
}

async fn post(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
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
