#![cfg(target_os = "linux")]
//! Scan-worker leases: lost scans are requeued at once with a bounded attempt
//! count, direct scans are never mistaken for lost ones, and a worker whose
//! claim was taken over never commits its catalog changes.
mod support;

use std::{fs, path::PathBuf, time::Duration};

use volundd::{scan_pipeline, scanner};

const SCAN_LOCK_NAMESPACE: i64 = 6_219_273_558_347_087_872;

struct Fixture {
    db: support::TestDatabase,
    root: PathBuf,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = support::test_database().await?;
        let root = std::env::temp_dir().join(format!(
            "volund-worker-lease-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("part.step"), b"leased scan original").unwrap();
        scanner::register_root(&db, "lease", "Lease", &root)
            .await
            .unwrap();
        scanner::scan_root(&db, "lease", false).await.unwrap();
        Some(Self { db, root })
    }

    /// A `running` scan row without any worker lease, as a killed worker leaves it.
    async fn orphan(&self, attempt: i32) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO volund.scan_runs (library_root_id,status,attempt,started_at) \
             SELECT id,'running',$1,now() FROM volund.library_roots RETURNING id",
        )
        .bind(attempt)
        .fetch_one(&*self.db)
        .await
        .unwrap()
    }

    async fn row(&self, id: i64) -> (String, i32, Option<String>) {
        sqlx::query_as("SELECT status,attempt,error_message FROM volund.scan_runs WHERE id=$1")
            .bind(id)
            .fetch_one(&*self.db)
            .await
            .unwrap()
    }

    async fn sources(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM volund.source_files")
            .fetch_one(&*self.db)
            .await
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn lost_scan_is_requeued_at_once_and_completes_with_a_counted_attempt() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let scan = f.orphan(1).await;
    let report = scan_pipeline::process_next(&f.db).await.unwrap().unwrap();
    assert_eq!(report.status, "completed");
    assert_eq!(f.row(scan).await, ("completed".into(), 2, None));
    assert_eq!(f.sources().await, 1);
}

#[tokio::test]
async fn repeatedly_lost_scan_fails_instead_of_requeueing_forever() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let scan = f.orphan(3).await;
    assert!(scan_pipeline::process_next(&f.db).await.unwrap().is_none());
    let (status, attempt, message) = f.row(scan).await;
    assert_eq!((status.as_str(), attempt), ("failed", 3));
    assert!(message.unwrap().contains("stopped repeatedly"));
}

#[tokio::test]
async fn direct_scan_holding_the_library_lock_is_not_recovered() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let scan = f.orphan(1).await;
    let root: i64 = sqlx::query_scalar("SELECT id FROM volund.library_roots")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let mut direct = f.db.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1 + $2)")
        .bind(SCAN_LOCK_NAMESPACE)
        .bind(root)
        .execute(&mut *direct)
        .await
        .unwrap();
    assert!(scan_pipeline::process_next(&f.db).await.unwrap().is_none());
    assert_eq!(f.row(scan).await.0, "running");
    sqlx::query("SELECT pg_advisory_unlock($1 + $2)")
        .bind(SCAN_LOCK_NAMESPACE)
        .bind(root)
        .execute(&mut *direct)
        .await
        .unwrap();
}

#[tokio::test]
async fn scan_whose_claim_was_taken_over_never_commits_catalog_changes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    fs::write(f.root.join("late.step"), b"must not be indexed").unwrap();
    let scan = f.orphan(1).await;
    let mut barrier = f.db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.source_files IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let pool = f.db.clone();
    let worker =
        tokio::spawn(async move { scanner::scan_queued(&pool, scan, "lease", false).await });
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
    .expect("scanner must reach source loading");
    // Another worker finished the run meanwhile, e.g. after recovering it.
    sqlx::query("UPDATE volund.scan_runs SET status='failed',finished_at=now() WHERE id=$1")
        .bind(scan)
        .execute(&*f.db)
        .await
        .unwrap();
    barrier.rollback().await.unwrap();
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.contains("claim was lost"), "{error}");
    assert_eq!(f.row(scan).await.0, "failed");
    assert_eq!(f.sources().await, 1, "the late scan must roll back");
}
