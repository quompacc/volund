#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::{Postgres, Transaction};
use volundd::database;

mod support;

use support::test_database;

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn unique_number() -> u64 {
    u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
}

async fn insert_content(transaction: &mut Transaction<'_, Postgres>) -> i64 {
    let unique = unique_number();
    let sha256 = format!("{unique:064x}");
    sqlx::query_scalar(
        "INSERT INTO volund.content_objects (sha256, byte_size, detected_format) \
         VALUES ($1, 128, 'step') RETURNING id",
    )
    .bind(sha256)
    .fetch_one(&mut **transaction)
    .await
    .expect("insert content")
}

async fn insert_root_and_scan(transaction: &mut Transaction<'_, Postgres>) -> (i64, i64) {
    let unique = unique_number();
    let root_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.library_roots \
         (root_key, display_name, filesystem_path) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("test_{unique}"))
    .bind(format!("Test {unique}"))
    .bind(format!("/tmp/volund-test-{unique}"))
    .fetch_one(&mut **transaction)
    .await
    .expect("insert library root");
    let scan_id = sqlx::query_scalar(
        "INSERT INTO volund.scan_runs (library_root_id, status) \
         VALUES ($1, 'running') RETURNING id",
    )
    .bind(root_id)
    .fetch_one(&mut **transaction)
    .await
    .expect("insert scan run");
    (root_id, scan_id)
}

#[tokio::test]
async fn migration_builds_the_expected_schema_and_is_idempotent() {
    let Some(pool) = test_database().await else {
        return;
    };
    let health = database::inspect(&pool).await.expect("inspect schema");
    assert!(health.is_ready());
    assert_eq!(
        database::run_migrations(&pool)
            .await
            .expect("repeat migration"),
        0
    );

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'volund' AND table_type = 'BASE TABLE' \
         ORDER BY table_name",
    )
    .fetch_all(&*pool)
    .await
    .expect("list schema tables");
    assert_eq!(
        tables,
        [
            "authors",
            "collection_models",
            "collections",
            "content_objects",
            "conversion_profiles",
            "conversion_runs",
            "derived_artifacts",
            "file_dependencies",
            "import_cleanup_runs",
            "import_draft_collections",
            "import_draft_items",
            "import_drafts",
            "instance_settings",
            "instance_state",
            "library_roots",
            "lifecycle_plans",
            "model_components",
            "model_source_files",
            "model_tags",
            "models",
            "operational_components",
            "operational_log_events",
            "password_credentials",
            "retention_runs",
            "scan_runs",
            "scan_schedules",
            "security_audit_events",
            "sessions",
            "slicer_handoffs",
            "source_cleanup",
            "source_file_moves",
            "source_files",
            "source_quarantines",
            "tags",
            "user_invitations",
            "user_preferences",
            "users",
        ]
    );
}

#[tokio::test]
async fn identity_schema_enforces_security_invariants() {
    let Some(pool) = test_database().await else {
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.users \
         (email, normalized_email, display_name, role, status) \
         VALUES ('Owner@example.test', 'owner@example.test', 'First Owner', 'owner', 'active') \
         RETURNING id",
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("insert owner");

    sqlx::query(
        "INSERT INTO volund.password_credentials (user_id, password_hash) \
         VALUES ($1, '$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA')",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .expect("insert Argon2id credential");
    sqlx::query(
        "UPDATE volund.instance_state SET initialized_at = now(), owner_user_id = $1 \
         WHERE singleton",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .expect("initialize singleton state");

    let second_state = sqlx::query("INSERT INTO volund.instance_state (singleton) VALUES (true)")
        .execute(&mut *transaction)
        .await;
    assert!(second_state.is_err());
    transaction.rollback().await.expect("rollback transaction");

    let mut transaction = pool.begin().await.expect("begin second transaction");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.users \
         (email, normalized_email, display_name, role, status) \
         VALUES ('Other@example.test', 'other@example.test', 'Other Owner', 'owner', 'active') \
         RETURNING id",
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("insert second owner fixture");
    let invalid_digest = sqlx::query(
        "INSERT INTO volund.sessions \
         (user_id, token_digest, csrf_digest, idle_expires_at, absolute_expires_at) \
         VALUES ($1, 'plaintext', $2, now() + interval '1 hour', now() + interval '1 day')",
    )
    .bind(user_id)
    .bind("a".repeat(64))
    .execute(&mut *transaction)
    .await;
    assert!(invalid_digest.is_err());
    transaction
        .rollback()
        .await
        .expect("rollback second transaction");
}

#[tokio::test]
async fn identical_content_hashes_are_rejected() {
    let Some(pool) = test_database().await else {
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let unique = unique_number();
    let sha256 = format!("{unique:064x}");
    sqlx::query(
        "INSERT INTO volund.content_objects (sha256, byte_size, detected_format) \
         VALUES ($1, 128, 'step')",
    )
    .bind(&sha256)
    .execute(&mut *transaction)
    .await
    .expect("insert first content");
    let duplicate = sqlx::query(
        "INSERT INTO volund.content_objects (sha256, byte_size, detected_format) \
         VALUES ($1, 128, 'step')",
    )
    .bind(sha256)
    .execute(&mut *transaction)
    .await;
    assert!(duplicate.is_err());
    transaction.rollback().await.expect("rollback transaction");
}

#[tokio::test]
async fn source_paths_cannot_escape_their_library_root() {
    let Some(pool) = test_database().await else {
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let (root_id, scan_id) = insert_root_and_scan(&mut transaction).await;
    let content_id = insert_content(&mut transaction).await;
    let invalid = sqlx::query(
        "INSERT INTO volund.source_files \
         (library_root_id, content_object_id, relative_path, \
          filesystem_modified_at, last_seen_scan_id) \
         VALUES ($1, $2, '../escape.step', now(), $3)",
    )
    .bind(root_id)
    .bind(content_id)
    .bind(scan_id)
    .execute(&mut *transaction)
    .await;
    assert!(invalid.is_err());
    transaction.rollback().await.expect("rollback transaction");
}

#[tokio::test]
async fn one_conversion_cannot_publish_duplicate_artifact_kinds() {
    let Some(pool) = test_database().await else {
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let content_id = insert_content(&mut transaction).await;
    let conversion_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs \
         (content_object_id, converter_name, converter_version, \
          contract_version, profile, status) \
         VALUES ($1, 'volund-cad-convert', $2, 1, 'web', 'ready') \
         RETURNING id",
    )
    .bind(content_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .fetch_one(&mut *transaction)
    .await
    .expect("insert conversion");
    let hash = "a".repeat(64);
    for attempt in 0..2 {
        let result = sqlx::query(
            "INSERT INTO volund.derived_artifacts \
             (conversion_run_id, artifact_kind, relative_path, sha256, \
              byte_size, media_type) \
             VALUES ($1, 'preview-glb', 'preview.glb', $2, 256, 'model/gltf-binary')",
        )
        .bind(conversion_id)
        .bind(&hash)
        .execute(&mut *transaction)
        .await;
        assert_eq!(result.is_ok(), attempt == 0);
    }
    transaction.rollback().await.expect("rollback transaction");
}

#[tokio::test]
async fn cad_files_can_resolve_hashed_non_cad_sidecars() {
    let Some(pool) = test_database().await else {
        return;
    };
    let mut transaction = pool.begin().await.expect("begin transaction");
    let (root_id, scan_id) = insert_root_and_scan(&mut transaction).await;
    let cad_content_id = insert_content(&mut transaction).await;
    let sidecar_hash = format!("{:064x}", unique_number());
    let sidecar_content_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.content_objects \
         (sha256, byte_size, detected_format, media_type) \
         VALUES ($1, 512, NULL, 'application/octet-stream') RETURNING id",
    )
    .bind(sidecar_hash)
    .fetch_one(&mut *transaction)
    .await
    .expect("insert sidecar content");

    let cad_file_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.source_files \
         (library_root_id, content_object_id, relative_path, \
          filesystem_modified_at, last_seen_scan_id) \
         VALUES ($1, $2, 'assembly/model.gltf', now(), $3) RETURNING id",
    )
    .bind(root_id)
    .bind(cad_content_id)
    .bind(scan_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("insert CAD source file");
    let sidecar_file_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.source_files \
         (library_root_id, content_object_id, relative_path, \
          filesystem_modified_at, last_seen_scan_id) \
         VALUES ($1, $2, 'assembly/model.bin', now(), $3) RETURNING id",
    )
    .bind(root_id)
    .bind(sidecar_content_id)
    .bind(scan_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("insert sidecar source file");
    sqlx::query(
        "INSERT INTO volund.file_dependencies \
         (source_file_id, dependency_kind, raw_reference, resolved_source_file_id) \
         VALUES ($1, 'buffer', 'model.bin', $2)",
    )
    .bind(cad_file_id)
    .bind(sidecar_file_id)
    .execute(&mut *transaction)
    .await
    .expect("resolve sidecar dependency");
    transaction.rollback().await.expect("rollback transaction");
}
