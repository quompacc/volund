#![cfg(target_os = "linux")]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn library_changes_and_stale_retries_preserve_existing_catalog_data() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-existing-library-{suffix}"));
    std::fs::create_dir(&root).expect("create library fixture");
    let original = root.join("assembly.step");
    std::fs::write(&original, b"ISO-10303-21; protected original fixture")
        .expect("write original fixture");
    let original_hash = volundd::file_hash::sha256_file(&original).expect("hash original");
    create_catalog_fixture(&db, &root, original_hash.as_str()).await;
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let root_identity_before = root_identity_snapshot(&db).await;
    let catalog_before = catalog_snapshot(&db).await;

    let (status, disabled) = request(
        &router,
        json!({
            "expectedRevision": 1,
            "name": "Protected archive",
            "enabled": false,
            "confirmation": "UPDATE LIBRARY protected"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert_eq!(disabled["name"], "Protected archive");
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["revision"], 2);
    assert_eq!(disabled["fileCount"], 1);
    assert_eq!(disabled["missingFileCount"], 0);
    assert_eq!(root_identity_snapshot(&db).await, root_identity_before);
    assert_eq!(catalog_snapshot(&db).await, catalog_before);
    assert_eq!(
        volundd::file_hash::sha256_file(&original).expect("rehash disabled original"),
        original_hash
    );

    let root_after_disable = root_snapshot(&db).await;
    let (status, _) = request(
        &router,
        json!({
            "expectedRevision": 1,
            "name": "Stale overwrite",
            "enabled": true,
            "confirmation": "UPDATE LIBRARY protected"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(root_snapshot(&db).await, root_after_disable);
    assert_eq!(root_identity_snapshot(&db).await, root_identity_before);
    assert_eq!(catalog_snapshot(&db).await, catalog_before);
    assert_eq!(successful_update_audits(&db).await, 1);

    let (status, enabled) = request(
        &router,
        json!({
            "expectedRevision": 2,
            "enabled": true,
            "confirmation": "UPDATE LIBRARY protected"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["revision"], 3);
    assert_eq!(root_identity_snapshot(&db).await, root_identity_before);
    assert_eq!(catalog_snapshot(&db).await, catalog_before);
    assert_eq!(
        volundd::file_hash::sha256_file(&original).expect("rehash enabled original"),
        original_hash
    );
    assert_eq!(successful_update_audits(&db).await, 2);
    std::fs::remove_dir_all(root).expect("remove library fixture");
}

async fn create_catalog_fixture(db: &sqlx::PgPool, root: &std::path::Path, digest: &str) {
    sqlx::query(
        "WITH library AS (
           INSERT INTO volund.library_roots (root_key,display_name,filesystem_path)
           VALUES ('protected','Protected',$1) RETURNING id
         ), scan AS (
           INSERT INTO volund.scan_runs (library_root_id,status,discovered_files,hashed_files)
           SELECT id,'completed',1,1 FROM library RETURNING id,library_root_id
         ), content AS (
           INSERT INTO volund.content_objects (sha256,byte_size,detected_format)
           VALUES ($2,$3,'step') RETURNING id
         ), source AS (
           INSERT INTO volund.source_files
             (library_root_id,content_object_id,relative_path,filesystem_modified_at,last_seen_scan_id)
           SELECT scan.library_root_id,content.id,'assembly.step',now(),scan.id FROM scan,content
           RETURNING id
         ), model AS (
           INSERT INTO volund.models (slug,name,kind,thumbnail_kind,thumbnail_source_file_id)
           SELECT 'protected-model','Protected model','assembly','source-file',source.id FROM source
           RETURNING id
         )
         INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary)
         SELECT model.id,source.id,'master-cad',true FROM model,source",
    )
    .bind(root.to_string_lossy().as_ref())
    .bind(digest)
    .bind(i64::try_from(std::fs::metadata(root.join("assembly.step")).unwrap().len()).unwrap())
    .execute(db)
    .await
    .expect("create protected catalog fixture");
}

async fn request(router: &Router, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/libraries/protected")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn root_snapshot(db: &sqlx::PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT to_jsonb(root) FROM volund.library_roots root WHERE root_key='protected'",
    )
    .fetch_one(db)
    .await
    .unwrap()
}

async fn root_identity_snapshot(db: &sqlx::PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
           'id',id,'rootKey',root_key,'filesystemPath',filesystem_path
         ) FROM volund.library_roots WHERE root_key='protected'",
    )
    .fetch_one(db)
    .await
    .unwrap()
}

async fn catalog_snapshot(db: &sqlx::PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
           'content', (SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id) FROM volund.content_objects row),
           'sources', (SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id) FROM volund.source_files row),
           'models', (SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id) FROM volund.models row),
           'links', (SELECT jsonb_agg(to_jsonb(row) ORDER BY row.model_id,row.source_file_id) FROM volund.model_source_files row)
         )",
    )
    .fetch_one(db)
    .await
    .unwrap()
}

async fn successful_update_audits(db: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events
         WHERE action='library.update' AND outcome='success'",
    )
    .fetch_one(db)
    .await
    .unwrap()
}
