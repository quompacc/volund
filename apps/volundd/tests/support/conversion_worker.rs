//! Shared real-process fixture for preview-worker recovery tests: a temporary
//! library with one source, a shell converter fixture, and process helpers.
#![allow(dead_code)] // Each integration-test crate uses a different subset.

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::{
    preview_pipeline::{self, PreviewWorkerConfig},
    scanner,
};

pub struct Fixture {
    pub db: super::TestDatabase,
    pub root: PathBuf,
    pub router: Router,
    pub job: String,
}

impl Fixture {
    pub async fn new() -> Option<Self> {
        let db = super::test_database().await?;
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
        let router = super::authenticated_router(&db, volundd::api::router(db.clone())).await;
        Some(Self {
            db,
            root,
            router,
            job,
        })
    }

    pub fn config(&self, mode: &str, timeout: u64) -> PreviewWorkerConfig {
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

    pub async fn action(&self, job: &str, action: &str) -> Value {
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

    pub async fn started(&self) -> Vec<u32> {
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

    pub async fn assert_terminal(&self, job: &str, status: &str, artifacts: i64) {
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

pub fn active(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| !fields.starts_with('Z'))
        })
        .unwrap_or(false)
}

pub fn stop(pid: u32) {
    if active(pid) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &pid.to_string()])
            .status();
    }
}

/// Wait until the database has ended the killed worker's session and its leases.
pub async fn leases_released(db: &sqlx::PgPool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let held: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE locktype='advisory' AND objsubid=2                  AND classid::bigint IN (860756369, 860756370)",
            )
            .fetch_one(db)
            .await
            .unwrap();
            if held == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("killed worker sessions must end");
}

pub fn process_group(pid: u32) -> u32 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat.rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .nth(2)
        .unwrap()
        .parse()
        .unwrap()
}

pub fn spawn_worker(config: &PreviewWorkerConfig) -> tokio::process::Child {
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
