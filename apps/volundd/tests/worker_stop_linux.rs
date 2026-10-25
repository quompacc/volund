#![cfg(target_os = "linux")]
//! Orderly worker stop: SIGTERM ends the converter's process group and hands
//! the interrupted job back to the queue, with a bounded number of requeues.
mod support;

use std::{fs, time::Duration};

use serde_json::Value;
use support::conversion_worker::{Fixture, active, spawn_worker, stop};
use volundd::{preview_pipeline, scan_pipeline};

fn terminate(child: &tokio::process::Child) {
    let pid = child.id().expect("worker is running");
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn sigterm_requeues_the_running_conversion_and_stops_the_converter() {
    for attempt in [1, 3] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        sqlx::query("UPDATE volund.conversion_runs SET attempt=$2 WHERE public_id::text=$1")
            .bind(&f.job)
            .bind(attempt)
            .execute(&*f.db)
            .await
            .unwrap();
        let mut worker = spawn_worker(&f.config("slow", 30));
        let pids = f.started().await;
        terminate(&worker);
        let exit = tokio::time::timeout(Duration::from_secs(10), worker.wait())
            .await
            .expect("worker must stop promptly")
            .unwrap();
        assert!(exit.success(), "an orderly stop is not a worker failure");
        let stopped = tokio::time::timeout(Duration::from_secs(2), async {
            while pids.iter().any(|pid| active(*pid)) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok();
        for pid in pids {
            stop(pid);
        }
        assert!(stopped, "stopped worker left converter processes running");
        assert!(!f.root.join("published").exists());
        let (status, now, diagnostics): (String, i32, Value) = sqlx::query_as(
            "SELECT status,attempt,diagnostics FROM volund.conversion_runs WHERE public_id::text=$1",
        )
        .bind(&f.job)
        .fetch_one(&*f.db)
        .await
        .unwrap();
        if attempt == 1 {
            assert_eq!((status.as_str(), now), ("queued", 2));
            assert!(diagnostics.to_string().contains("requeued"));
            let report = preview_pipeline::process_next(&f.db, &f.config("ready", 10))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(report.id, f.job);
            f.assert_terminal(&f.job, "ready", 5).await;
        } else {
            assert_eq!((status.as_str(), now), ("failed", 3));
            assert!(diagnostics.to_string().contains("stopped repeatedly"));
            f.assert_terminal(&f.job, "failed", 0).await;
        }
    }
}

#[tokio::test]
async fn sigterm_requeues_a_running_scan_without_catalog_changes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    fs::write(
        f.root.join("library/late.step"),
        b"must wait for the requeued scan",
    )
    .unwrap();
    let scan: i64 = sqlx::query_scalar(
        "INSERT INTO volund.scan_runs (library_root_id,status) SELECT id,'queued' FROM volund.library_roots RETURNING id",
    )
    .fetch_one(&*f.db)
    .await
    .unwrap();
    let mut barrier = f.db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.source_files IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let mut worker = tokio::process::Command::new(env!("CARGO_BIN_EXE_volundd"))
        .arg("process-next-scan")
        .env(
            "VOLUND_DATABASE_URL",
            std::env::var("VOLUND_TEST_DATABASE_URL").unwrap(),
        )
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() \
                 AND wait_event_type='Lock' AND query LIKE 'SELECT source.relative_path, source.content_object_id%')",
            )
            .fetch_one(&*f.db)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("scan worker must reach source loading");
    terminate(&worker);
    let exit = tokio::time::timeout(Duration::from_secs(10), worker.wait())
        .await
        .expect("scan worker must stop promptly")
        .unwrap();
    barrier.rollback().await.unwrap();
    assert!(exit.success());
    let (status, attempt): (String, i32) =
        sqlx::query_as("SELECT status,attempt FROM volund.scan_runs WHERE id=$1")
            .bind(scan)
            .fetch_one(&*f.db)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempt), ("queued", 2));
    let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.source_files")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    assert_eq!(sources, 1, "the interrupted scan must not persist anything");
    // The stopped worker's library lock is gone, so the requeued scan completes.
    let report = scan_pipeline::process_next(&f.db).await.unwrap().unwrap();
    assert_eq!(report.status, "completed");
}
