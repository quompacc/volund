#![cfg(target_os = "linux")]
//! Retention removes conversion directories that no catalog row references and
//! that are old enough not to belong to a live conversion; it keeps everything
//! else, including symlinks and foreign names.
mod support;

use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

use support::test_database;
use volundd::retention;

fn directory(path: &Path, age: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("job.json"), b"{}").unwrap();
    assert!(
        Command::new("touch")
            .args(["-d", age])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn retention_reaps_only_aged_unreferenced_conversion_directories() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = std::env::temp_dir().join(format!(
        "volund-reaper-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = base.join("derived");
    let outside = base.join("outside");
    directory(&outside, "10 days ago");
    directory(&root.join(".work/1000-1"), "3 days ago");
    directory(&root.join(".work/1000-2"), "1 hour ago");
    directory(&root.join("failed/1000-3"), "400 days ago");
    directory(&root.join("failed/1000-4"), "1 day ago");
    directory(&root.join("jobs/1000-5"), "2 days ago");
    directory(&root.join("jobs/1000-6"), "2 days ago");
    directory(&root.join("jobs/1000-7"), "1 hour ago");
    directory(&root.join("jobs/keep-me"), "400 days ago");
    symlink(&outside, root.join("jobs/1000-8")).unwrap();

    let content: i64 = sqlx::query_scalar(
        "INSERT INTO volund.content_objects (sha256,byte_size) VALUES ($1,1) RETURNING id",
    )
    .bind("a".repeat(64))
    .fetch_one(&*pool)
    .await
    .unwrap();
    let run: i64 = sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,\
         contract_version,profile,status,started_at,finished_at) \
         VALUES ($1,'volund-cad-convert','test',1,'web','ready',now(),now()) RETURNING id",
    )
    .bind(content)
    .fetch_one(&*pool)
    .await
    .unwrap();
    fs::write(root.join("jobs/1000-6/preview.glb"), b"glb").unwrap();
    sqlx::query(
        "INSERT INTO volund.derived_artifacts (conversion_run_id,artifact_kind,relative_path,\
         sha256,byte_size,media_type) VALUES ($1,'preview-glb','jobs/1000-6/preview.glb',$2,3,\
         'model/gltf-binary')",
    )
    .bind(run)
    .bind("0".repeat(64))
    .execute(&*pool)
    .await
    .unwrap();

    let result = retention::execute(&pool, &root, None, None).await.unwrap();
    assert_eq!(result.orphan_directories, 3, "{result:?}");
    assert!(result.orphan_bytes >= 6);
    assert_eq!(result.status, "completed", "{result:?}");
    for removed in [".work/1000-1", "failed/1000-3", "jobs/1000-5"] {
        assert!(!root.join(removed).exists(), "{removed} must be reaped");
    }
    for kept in [
        ".work/1000-2",
        "failed/1000-4",
        "jobs/1000-6",
        "jobs/1000-7",
        "jobs/keep-me",
        "jobs/1000-8",
    ] {
        assert!(
            fs::symlink_metadata(root.join(kept)).is_ok(),
            "{kept} must be kept"
        );
    }
    assert!(
        outside.join("job.json").exists(),
        "symlink targets are never touched"
    );
    fs::remove_dir_all(base).unwrap();
}
