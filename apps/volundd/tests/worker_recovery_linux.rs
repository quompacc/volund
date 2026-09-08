#![cfg(target_os = "linux")]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tower::ServiceExt;
use volundd::{
    preview_pipeline::{self, PreviewWorkerConfig},
    scanner,
};

struct Fixture {
    db: support::TestDatabase,
    root: PathBuf,
    router: Router,
    job: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = support::test_database().await?;
        let root = std::env::temp_dir().join(format!(
            "volund-worker-recovery-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("library")).unwrap();
        fs::write(root.join("library/part.step"), b"unchanged worker original").unwrap();
        scanner::register_root(&db, "probe", "Probe", &root.join("library"))
            .await
            .unwrap();
        scanner::scan_root(&db, "probe", false).await.unwrap();
        let id: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
            .fetch_one(&*db)
            .await
            .unwrap();
        let job = preview_pipeline::enqueue(&db, &id, "web").await.unwrap().id;
        let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
        Some(Self {
            db,
            root,
            router,
            job,
        })
    }

    fn config(&self, mode: &str, timeout: u64) -> PreviewWorkerConfig {
        let converter = self.root.join("converter.sh");
        let action = match mode {
            "slow" => "sleep 20 &\necho $! > \"$base/child.pid\"\nwait\n",
            "fail" => "exit 42\n",
            _ => "",
        };
        fs::write(&converter, format!("#!/bin/sh\nset -eu\nbase='{}'\necho $$ > \"$base/converter.pid\"\n{action}out=\nwhile [ \"$#\" -gt 0 ]; do\nif [ \"$1\" = --output ]; then out=$2; shift 2; else shift; fi\ndone\nprintf glb > \"$out/preview.glb\"\nprintf png > \"$out/thumbnail.png\"\nprintf '{{}}' > \"$out/assembly.json\"\nprintf '[]' > \"$out/diagnostics.json\"\nprintf '{{}}' > \"$out/result.json\"\ntouch \"$base/published\"\n", self.root.display())).unwrap();
        fs::set_permissions(&converter, fs::Permissions::from_mode(0o750)).unwrap();
        PreviewWorkerConfig::new(
            PathBuf::from(env!("CARGO_BIN_EXE_volundd")),
            converter,
            self.root.join("derived"),
            self.root.join("scratch"),
            timeout,
        )
        .unwrap()
    }

    async fn action(&self, job: &str, action: &str) -> Value {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/jobs/conversion/{job}/{action}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"confirmation":format!("{} {job}", action.to_uppercase())})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap()
    }

    async fn started(&self) -> Vec<u32> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let pids: Option<Vec<u32>> = ["converter.pid", "child.pid"]
                    .iter()
                    .map(|name| {
                        fs::read_to_string(self.root.join(name))
                            .ok()?
                            .trim()
                            .parse()
                            .ok()
                    })
                    .collect();
                if let Some(pids) = pids {
                    return pids;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("real converter and child must start")
    }

    async fn assert_terminal(&self, job: &str, status: &str, artifacts: i64) {
        let row: (String, bool, i64) = sqlx::query_as(
            "SELECT status,finished_at IS NOT NULL,(SELECT count(*) FROM volund.derived_artifacts WHERE conversion_run_id=run.id)
             FROM volund.conversion_runs run WHERE public_id::text=$1",
        ).bind(job).fetch_one(&*self.db).await.unwrap();
        assert_eq!(row, (status.into(), true, artifacts));
        assert_eq!(
            fs::read(self.root.join("library/part.step")).unwrap(),
            b"unchanged worker original"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["child.pid", "converter.pid"] {
            if let Ok(pid) = fs::read_to_string(self.root.join(name)) {
                if let Ok(pid) = pid.trim().parse::<u32>() {
                    stop(pid);
                }
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn active(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| !fields.starts_with('Z'))
        })
        .unwrap_or(false)
}

fn stop(pid: u32) {
    if active(pid) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &pid.to_string()])
            .status();
    }
}

#[tokio::test]
async fn restarted_worker_recovers_expired_claims_and_preserves_cancellation() {
    for cancel in [false, true] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let config = f.config("slow", 30);
        let mut worker = spawn_worker(&config);
        let pids = f.started().await;
        let stat = fs::read_to_string(format!("/proc/{}/stat", pids[0])).unwrap();
        let group: u32 = stat
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .nth(2)
            .unwrap()
            .parse()
            .unwrap();
        // Model a stopped service's whole process tree, using only this fixture's
        // observed process group. No production service is restarted.
        worker.kill().await.unwrap();
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .status();
        for pid in pids {
            stop(pid);
        }
        if cancel {
            f.action(&f.job, "cancel").await;
        }
        let config = f.config("ready", 10);
        assert!(spawn_worker(&config).wait().await.unwrap().success());
        let status = volundd::job_admin::get(&f.db, "conversion", &f.job)
            .await
            .unwrap();
        assert_eq!(
            status.status, "running",
            "fresh claims must not be stolen on restart"
        );
        // Exercise the real expiry query without a two-hour wall-clock sleep.
        sqlx::query("UPDATE volund.conversion_runs SET requested_at=now()-interval '4 hours',started_at=now()-interval '3 hours' WHERE public_id::text=$1")
            .bind(&f.job).execute(&*f.db).await.unwrap();
        assert!(spawn_worker(&config).wait().await.unwrap().success());
        f.assert_terminal(&f.job, if cancel { "cancelled" } else { "failed" }, 0)
            .await;
        let retry = f.action(&f.job, "retry").await;
        assert_eq!(retry["attempt"], 2);
        assert_eq!(retry["retryOfId"], f.job);
        assert!(spawn_worker(&config).wait().await.unwrap().success());
        f.assert_terminal(retry["id"].as_str().unwrap(), "ready", 5)
            .await;
    }
}

fn spawn_worker(config: &PreviewWorkerConfig) -> tokio::process::Child {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_volundd"))
        .arg("process-next-preview")
        .env(
            "VOLUND_DATABASE_URL",
            std::env::var("VOLUND_TEST_DATABASE_URL").unwrap(),
        )
        .env("VOLUND_CONVERTER", &config.converter)
        .env("VOLUND_DERIVED_ROOT", &config.derived_root)
        .env("VOLUND_SCRATCH_ROOT", &config.scratch_root)
        .env(
            "VOLUND_CONVERSION_TIMEOUT_SECONDS",
            config.timeout_seconds.to_string(),
        )
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

#[tokio::test]
async fn running_scan_observes_cancellation_before_catalog_mutation() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    fs::write(
        f.root.join("library/new.step"),
        b"must not be indexed by cancelled scan",
    )
    .unwrap();
    let (scan, public): (i64, String) = sqlx::query_as(
        "INSERT INTO volund.scan_runs (library_root_id,status) SELECT id,'running' FROM volund.library_roots RETURNING id,public_id::text",
    ).fetch_one(&*f.db).await.unwrap();
    let mut barrier = f.db.begin().await.unwrap();
    sqlx::query("LOCK TABLE volund.source_files IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let pool = f.db.clone();
    let scanner =
        tokio::spawn(async move { scanner::scan_queued(&pool, scan, "probe", false).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT source.relative_path, source.content_object_id%')",
            ).fetch_one(&*f.db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("scanner must enter actual source loading");
    let router = f.router.clone();
    let cancel = tokio::spawn(async move {
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/jobs/scan/{public}/cancel"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"confirmation":format!("CANCEL {public}")}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    });
    // Cancellation commits before its unified response query needs the locked
    // source table. Observe that commit before releasing the scanner.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let requested: bool = sqlx::query_scalar(
                "SELECT cancellation_requested_at IS NOT NULL FROM volund.scan_runs WHERE id=$1",
            )
            .bind(scan)
            .fetch_one(&*f.db)
            .await
            .unwrap();
            if requested {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    barrier.rollback().await.unwrap();
    assert_eq!(cancel.await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        scanner.await.unwrap().unwrap_err(),
        "scan cancellation requested"
    );
    let row: (String, bool, i64) = sqlx::query_as(
        "SELECT status,finished_at IS NOT NULL,(SELECT count(*) FROM volund.source_files) FROM volund.scan_runs WHERE id=$1",
    ).bind(scan).fetch_one(&*f.db).await.unwrap();
    assert_eq!(row, ("cancelled".into(), true, 1));
    assert_eq!(
        fs::read(f.root.join("library/part.step")).unwrap(),
        b"unchanged worker original"
    );
}

#[tokio::test]
async fn cancellation_stops_the_real_converter_and_its_child() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let config = f.config("slow", 30);
    let pool = f.db.clone();
    let worker = tokio::spawn(async move { preview_pipeline::process_next(&pool, &config).await });
    let pids = f.started().await;
    let requested = f.action(&f.job, "cancel").await;
    assert!(requested["cancellationRequestedAtUnixMs"].is_number());
    let report = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(report.status, "cancelled");
    f.assert_terminal(&f.job, "cancelled", 0).await;
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
    assert!(stopped, "cancelled job left converter processes running");
    assert!(!f.root.join("published").exists());
    let retry = f.action(&f.job, "retry").await;
    let report = preview_pipeline::process_next(&f.db, &f.config("ready", 10))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.id, retry["id"].as_str().unwrap());
    f.assert_terminal(&report.id, "ready", 5).await;
}

#[tokio::test]
async fn converter_failure_and_timeout_are_terminal_and_retryable() {
    for mode in ["fail", "slow"] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let report = tokio::time::timeout(
            Duration::from_secs(10),
            preview_pipeline::process_next(&f.db, &f.config(mode, 1)),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert_eq!(report.status, "failed");
        f.assert_terminal(&f.job, "failed", 0).await;
        if mode == "slow" {
            let pids = f.started().await;
            tokio::time::timeout(Duration::from_secs(2), async {
                while pids.iter().any(|pid| active(*pid)) {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("timeout must stop all converter processes");
        }
        let diagnostics: Value = sqlx::query_scalar(
            "SELECT diagnostics FROM volund.conversion_runs WHERE public_id::text=$1",
        )
        .bind(&f.job)
        .fetch_one(&*f.db)
        .await
        .unwrap();
        assert!(diagnostics.to_string().contains(if mode == "slow" {
            "exceeded the 1 second limit"
        } else {
            "42"
        }));
        let retry = f.action(&f.job, "retry").await;
        let duplicate = f.action(&f.job, "retry").await;
        assert_eq!(retry["id"], duplicate["id"]);
        let report = preview_pipeline::process_next(&f.db, &f.config("ready", 10))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.id, retry["id"].as_str().unwrap());
        f.assert_terminal(&report.id, "ready", 5).await;
    }
}

#[tokio::test]
async fn expired_scan_claims_resume_or_cancel_without_duplicate_sources() {
    for cancel in [false, true] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let source: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
            .fetch_one(&*f.db)
            .await
            .unwrap();
        let scan: String = sqlx::query_scalar(
            "INSERT INTO volund.scan_runs (library_root_id,status,requested_at,started_at,cancellation_requested_at)
             SELECT id,'running',now()-interval '4 hours',now()-interval '3 hours',
             CASE WHEN $1 THEN now() END FROM volund.library_roots RETURNING public_id::text",
        ).bind(cancel).fetch_one(&*f.db).await.unwrap();
        let report = volundd::scan_pipeline::process_next(&f.db).await.unwrap();
        if cancel {
            assert!(report.is_none());
        } else {
            let report = report.unwrap();
            assert_eq!(report.id, scan);
            assert_eq!(report.status, "completed");
        }
        let status = volundd::job_admin::get(&f.db, "scan", &scan).await.unwrap();
        assert_eq!(
            status.status,
            if cancel { "cancelled" } else { "completed" }
        );
        assert!(status.finished_at_unix_ms.is_some());
        let sources: Vec<String> =
            sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
                .fetch_all(&*f.db)
                .await
                .unwrap();
        assert_eq!(sources, vec![source]);
    }
}
