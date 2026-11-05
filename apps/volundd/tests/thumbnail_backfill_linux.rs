#![cfg(target_os = "linux")]
//! Automatic thumbnail backfill (ADR 0040): which models get a conversion,
//! which source is chosen, throttling, and no automatic retry after failure.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use volundd::{policy_worker, scanner, thumbnail_backfill};

mod support;

use support::test_database;

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn create_model(pool: &PgPool, slug: &str, links: &[(&str, &str, bool)]) {
    sqlx::query("INSERT INTO volund.models (slug,name,kind) VALUES ($1,$1,'part')")
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert model");
    for (ordinal, (path, role, primary)) in links.iter().enumerate() {
        sqlx::query(
            "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal) \
             SELECT model.id,source.id,$3,$4,$5 FROM volund.models model, volund.source_files source \
             WHERE model.slug=$1 AND source.relative_path=$2",
        )
        .bind(slug)
        .bind(path)
        .bind(role)
        .bind(primary)
        .bind(i32::try_from(ordinal).expect("ordinal"))
        .execute(pool)
        .await
        .expect("link model file");
    }
}

/// Statuses of conversions requested for the file at `path`, oldest first.
async fn runs(pool: &PgPool, path: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT run.status FROM volund.conversion_runs run \
         JOIN volund.source_files source ON source.content_object_id=run.content_object_id \
         WHERE source.relative_path=$1 ORDER BY run.id",
    )
    .bind(path)
    .fetch_all(pool)
    .await
    .expect("conversion runs")
}

async fn finish_active(pool: &PgPool, status: &str) {
    sqlx::query(
        "UPDATE volund.conversion_runs SET status=$1, started_at=requested_at, finished_at=requested_at \
         WHERE status IN ('queued','running')",
    )
    .bind(status)
    .execute(pool)
    .await
    .expect("finish active conversions");
}

#[tokio::test]
async fn placeholder_models_get_one_throttled_conversion_without_retry_loops() {
    let Some(pool) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-thumbnail-backfill-{suffix}"));
    fs::create_dir_all(&root).expect("create fixture root");
    for (name, bytes) in [
        ("part.STL", "solid part"),
        ("assembly.step", "ISO-10303-21; assembly"),
        ("spare.stl", "solid spare"),
        ("notes.pdf", "%PDF notes"),
        ("cover.png", "png bytes"),
    ] {
        fs::write(root.join(name), bytes).expect("write fixture file");
    }
    scanner::register_root(&pool, "backfill_test", "Backfill Test", &root)
        .await
        .expect("register root");
    scanner::scan_root(&pool, "backfill_test", false)
        .await
        .expect("scan fixture");

    create_model(
        &pool,
        "mesh-only",
        &[
            ("part.STL", "printable-mesh", false),
            ("notes.pdf", "document", false),
        ],
    )
    .await;
    create_model(
        &pool,
        "cad-first",
        &[
            ("spare.stl", "printable-mesh", false),
            ("assembly.step", "master-cad", true),
        ],
    )
    .await;
    create_model(&pool, "documents-only", &[("notes.pdf", "document", false)]).await;
    create_model(
        &pool,
        "chosen-image",
        &[
            ("spare.stl", "printable-mesh", false),
            ("cover.png", "image", false),
        ],
    )
    .await;
    sqlx::query(
        "UPDATE volund.models SET thumbnail_kind='source-file', thumbnail_source_file_id=\
         (SELECT id FROM volund.source_files WHERE relative_path='cover.png') WHERE slug='chosen-image'",
    )
    .execute(&*pool)
    .await
    .expect("choose own image");

    // No free conversion slot: nothing is queued.
    assert_eq!(thumbnail_backfill::enqueue_missing(&pool, 0).await, Ok(0));
    // One conversion per placeholder model; the primary CAD file wins over a mesh,
    // upper-case extensions count, documents-only and chosen images are skipped.
    assert_eq!(thumbnail_backfill::enqueue_missing(&pool, 10).await, Ok(2));
    assert_eq!(runs(&pool, "part.STL").await, ["queued"]);
    assert_eq!(runs(&pool, "assembly.step").await, ["queued"]);
    assert!(runs(&pool, "spare.stl").await.is_empty());
    // While a model converts it is not queued again.
    assert_eq!(thumbnail_backfill::enqueue_missing(&pool, 10).await, Ok(0));

    // Failures with this converter version are not retried; the CAD model
    // falls back to its next renderable file, the mesh-only model has none.
    finish_active(&pool, "failed").await;
    assert_eq!(policy_worker::run_scheduler(&pool).await, Ok(1));
    assert_eq!(runs(&pool, "spare.stl").await, ["queued"]);
    assert_eq!(runs(&pool, "part.STL").await, ["failed"]);

    // A ready run with a thumbnail completes the model for this version.
    finish_active(&pool, "ready").await;
    sqlx::query(
        "INSERT INTO volund.derived_artifacts (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type) \
         SELECT id,'thumbnail-raster','jobs/test/thumbnail.png',repeat('a',64),1,'image/png' \
         FROM volund.conversion_runs WHERE status='ready'",
    )
    .execute(&*pool)
    .await
    .expect("record generated thumbnail");
    assert_eq!(thumbnail_backfill::enqueue_missing(&pool, 10).await, Ok(0));
    let versions: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT converter_version FROM volund.conversion_runs")
            .fetch_all(&*pool)
            .await
            .expect("converter versions");
    assert_eq!(versions, [VERSION]);
    fs::remove_dir_all(root).expect("remove fixture root");
}
