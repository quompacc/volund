#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::Row;
use volundd::scanner;

mod support;

use support::test_database;

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn unique_fixture() -> (String, PathBuf) {
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    (
        format!("scan_{unique}"),
        PathBuf::from(format!("/tmp/volund-scanner-{unique}")),
    )
}

async fn link_copy_to_model(pool: &sqlx::PgPool, root_key: &str) -> (i64, i64) {
    let source: i64 = sqlx::query_scalar(
        "SELECT source.id FROM volund.source_files source
         JOIN volund.library_roots root ON root.id=source.library_root_id
         WHERE root.root_key=$1 AND source.relative_path='copy.STP'",
    )
    .bind(root_key)
    .fetch_one(pool)
    .await
    .expect("load stable source identity");
    let model: i64 = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind)
         VALUES ($1,'Scanner-linked model','part') RETURNING id",
    )
    .bind(format!("scanner-linked-{source}"))
    .fetch_one(pool)
    .await
    .expect("create linked model");
    sqlx::query(
        "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary)
         VALUES ($1,$2,'master-cad',true)",
    )
    .bind(model)
    .bind(source)
    .execute(pool)
    .await
    .expect("link stable source identity");
    (source, model)
}

async fn restored_source(pool: &sqlx::PgPool, root_key: &str, model: i64) -> (i64, bool, i64) {
    sqlx::query_as(
        "SELECT source.id,source.missing_at IS NULL,
                (SELECT count(*) FROM volund.model_source_files link
                 WHERE link.model_id=$2 AND link.source_file_id=source.id)
         FROM volund.source_files source
         JOIN volund.library_roots root ON root.id=source.library_root_id
         WHERE root.root_key=$1 AND source.relative_path='copy.STP'",
    )
    .bind(root_key)
    .bind(model)
    .fetch_one(pool)
    .await
    .expect("inspect restored source identity")
}

#[tokio::test]
async fn scanner_is_incremental_deduplicating_and_marks_missing_paths() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(root.join("nested")).expect("create fixture directories");
    fs::write(root.join("part.step"), b"identical CAD bytes").expect("write STEP fixture");
    fs::write(root.join("copy.STP"), b"identical CAD bytes").expect("write duplicate fixture");
    fs::write(root.join("nested/model.glb"), b"glb fixture").expect("write GLB fixture");
    fs::write(root.join("notes.txt"), b"project notes").expect("write document fixture");
    let outside = root.with_extension("outside.step");
    fs::write(&outside, b"must not be followed").expect("write symlink target");
    symlink(&outside, root.join("linked.step")).expect("create symlink fixture");

    let canonical = scanner::register_root(&pool, &root_key, "Scanner Test", &root)
        .await
        .expect("register root");
    assert_eq!(
        canonical,
        fs::canonicalize(&root).expect("canonical fixture")
    );
    scanner::register_root(&pool, &root_key, "Renamed Scanner Test", &root)
        .await
        .expect("registration is idempotent for the same path");

    let first = scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("first scan");
    assert_eq!(first.discovered_files, 4);
    assert_eq!(first.hashed_files, 4);
    assert_eq!(first.missing_files, 0);

    let row = sqlx::query(
        "SELECT count(*)::bigint, count(DISTINCT source.content_object_id)::bigint \
         FROM volund.source_files source WHERE source.library_root_id = \
         (SELECT id FROM volund.library_roots WHERE root_key = $1)",
    )
    .bind(&root_key)
    .fetch_one(&*pool)
    .await
    .expect("count indexed files");
    assert_eq!(row.get::<i64, _>(0), 4);
    assert_eq!(row.get::<i64, _>(1), 3);
    let (copy_source, model) = link_copy_to_model(&pool, &root_key).await;

    let incremental = scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("incremental scan");
    assert_eq!(incremental.hashed_files, 0);
    let full = scanner::scan_root(&pool, &root_key, true)
        .await
        .expect("full scan");
    assert_eq!(full.hashed_files, 4);

    fs::write(
        root.join("part.step"),
        b"changed CAD bytes with another size",
    )
    .expect("change STEP fixture");
    fs::remove_file(root.join("copy.STP")).expect("remove duplicate fixture");
    let changed = scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("changed scan");
    assert_eq!(changed.discovered_files, 3);
    assert_eq!(changed.hashed_files, 1);
    assert_eq!(changed.missing_files, 1);

    let missing: bool = sqlx::query_scalar(
        "SELECT missing_at IS NOT NULL FROM volund.source_files \
         WHERE library_root_id = (SELECT id FROM volund.library_roots WHERE root_key = $1) \
         AND relative_path = 'copy.STP'",
    )
    .bind(&root_key)
    .fetch_one(&*pool)
    .await
    .expect("inspect missing path");
    assert!(missing);

    fs::write(root.join("copy.STP"), b"identical CAD bytes").expect("restore missing fixture");
    fs::write(root.join("new.iges"), b"new CAD bytes").expect("write new fixture");
    let returned = scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("returning and new file scan");
    assert_eq!(returned.discovered_files, 5);
    assert_eq!(returned.hashed_files, 2);
    assert_eq!(returned.missing_files, 0);
    let restored = restored_source(&pool, &root_key, model).await;
    assert_eq!(restored, (copy_source, true, 1));
    assert_eq!(
        fs::read(&outside).expect("read symlink target"),
        b"must not be followed"
    );

    fs::remove_dir_all(&root).expect("remove fixture root");
    fs::remove_file(outside).expect("remove outside fixture");
}

#[tokio::test]
async fn failed_scan_is_recorded_without_marking_existing_files_missing() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(&root).expect("create fixture root");
    fs::write(root.join("part.stl"), b"solid fixture").expect("write fixture");
    scanner::register_root(&pool, &root_key, "Failure Test", &root)
        .await
        .expect("register root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("initial scan");
    fs::remove_dir_all(&root).expect("make root unavailable");

    assert!(scanner::scan_root(&pool, &root_key, false).await.is_err());
    let row = sqlx::query(
        "SELECT run.status, source.missing_at IS NULL \
         FROM volund.scan_runs run \
         JOIN volund.library_roots root ON root.id = run.library_root_id \
         JOIN volund.source_files source ON source.library_root_id = root.id \
         WHERE root.root_key = $1 ORDER BY run.started_at DESC LIMIT 1",
    )
    .bind(&root_key)
    .fetch_one(&*pool)
    .await
    .expect("inspect failed scan");
    assert_eq!(row.get::<String, _>(0), "failed");
    assert!(row.get::<bool, _>(1));
}

#[tokio::test]
async fn concurrent_scan_of_the_same_root_is_refused() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(&root).expect("create fixture root");
    scanner::register_root(&pool, &root_key, "Lock Test", &root)
        .await
        .expect("register root");
    let root_id: i64 =
        sqlx::query_scalar("SELECT id FROM volund.library_roots WHERE root_key = $1")
            .bind(&root_key)
            .fetch_one(&*pool)
            .await
            .expect("load root id");
    let mut lock_connection = pool.acquire().await.expect("acquire lock connection");
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1 + $2)")
        .bind(6_219_273_558_347_087_872_i64)
        .bind(root_id)
        .fetch_one(&mut *lock_connection)
        .await
        .expect("hold scanner lock");
    assert!(locked);

    let error = scanner::scan_root(&pool, &root_key, false)
        .await
        .expect_err("parallel scan must fail");
    assert!(error.contains("already running"));
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1 + $2)")
        .bind(6_219_273_558_347_087_872_i64)
        .bind(root_id)
        .fetch_one(&mut *lock_connection)
        .await
        .expect("release scanner lock");
    assert!(unlocked);
    fs::remove_dir_all(root).expect("remove fixture root");
}
