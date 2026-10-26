#![cfg(target_os = "linux")]
//! Library onboarding dry run and scan progress visibility (ADR 0035).
mod support;

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, get_json, test_database};
use tower::ServiceExt;
use volundd::scanner;

async fn post(router: &axum::Router, uri: &str, body: &Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One prospective library covers every dry-run finding.
async fn dry_run_reports_what_a_first_scan_would_do_without_changing_anything() {
    let Some(pool) = test_database().await else {
        return;
    };
    let base = std::env::temp_dir().join(format!("volund-onboarding-{}", std::process::id()));
    let candidate = base.join("candidate");
    fs::create_dir_all(candidate.join("parts/inner")).unwrap();
    fs::write(candidate.join("parts/frame.step"), vec![b's'; 3000]).unwrap();
    fs::write(candidate.join("parts/inner/clip.STL"), vec![b'm'; 1000]).unwrap();
    fs::write(candidate.join("readme.txt"), b"notes").unwrap();
    fs::write(candidate.join(OsStr::from_bytes(b"bad\xff.step")), b"x").unwrap();
    symlink(candidate.join("readme.txt"), candidate.join("link.txt")).unwrap();
    fs::create_dir_all(candidate.join("locked")).unwrap();
    fs::write(candidate.join("locked/hidden.step"), b"hidden").unwrap();
    fs::set_permissions(candidate.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    // Root (as in the CI container) reads it anyway; then it is an ordinary folder.
    let locked = fs::read_dir(candidate.join("locked")).is_err();
    fs::create_dir_all(candidate.join(".volund-quarantine")).unwrap();
    fs::write(
        candidate.join(".volund-quarantine/old.step"),
        b"quarantined",
    )
    .unwrap();
    // An already registered library inside the candidate.
    scanner::register_root(&pool, "inner", "Inner", &candidate.join("parts/inner"))
        .await
        .unwrap();
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;

    let (status, report) = post(
        &router,
        "/api/v1/libraries/dry-run",
        &json!({"path": candidate}),
    )
    .await;
    fs::set_permissions(candidate.join("locked"), fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(status, StatusCode::OK, "{report}");
    let hidden = u64::from(!locked);
    assert_eq!(report["files"].as_u64(), Some(3 + hidden), "{report}");
    assert_eq!(report["bytes"].as_u64(), Some(4005 + 6 * hidden));
    assert_eq!(
        (report["cadFiles"].as_u64(), report["otherFiles"].as_u64()),
        (Some(2 + hidden), Some(1))
    );
    assert_eq!(report["formats"], json!({"step": 1 + hidden, "stl": 1}));
    assert_eq!(report["symlinksSkipped"], 1);
    assert_eq!(
        report["largestFiles"][0],
        json!({"path": "parts/frame.step", "bytes": 3000})
    );
    let blockers: Vec<&str> = report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect();
    let mut sorted = blockers.clone();
    sorted.sort_unstable();
    let expected: &[&str] = if locked {
        &["directory_unreadable", "name_not_utf8"]
    } else {
        &["name_not_utf8"]
    };
    assert_eq!(sorted, expected, "{report}");
    assert_eq!(report["blockerCount"].as_u64(), Some(1 + u64::from(locked)));
    assert_eq!(
        report["overlaps"],
        json!([{"key": "inner", "path": candidate.join("parts/inner"), "relation": "contains"}])
    );
    assert_eq!(report["truncated"], false);
    let roots: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.library_roots")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(roots, 1, "a dry run registers nothing");

    let (status, _) = post(
        &router,
        "/api/v1/libraries/dry-run",
        &json!({"path": base.join("missing")}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(
        &router,
        "/api/v1/libraries/dry-run",
        &json!({"path": "relative/path"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A queued scan is listed with its id and progress, and cancellation shows.
    sqlx::query(
        "INSERT INTO volund.scan_runs (library_root_id,status,discovered_files,hashed_files) \
         SELECT id,'running',120,45 FROM volund.library_roots",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let (_, libraries) = get_json(&router, "/api/v1/libraries").await;
    let latest = &libraries[0]["latestScan"];
    assert_eq!(
        (
            latest["status"].as_str(),
            latest["discoveredFiles"].as_i64(),
            latest["hashedFiles"].as_i64()
        ),
        (Some("running"), Some(120), Some(45))
    );
    assert_eq!(latest["cancellationRequested"], false);
    let scan = latest["id"].as_str().unwrap().to_owned();
    let (status, _) = post(
        &router,
        &format!("/api/v1/jobs/scan/{scan}/cancel"),
        &json!({"confirmation": format!("CANCEL {scan}")}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, libraries) = get_json(&router, "/api/v1/libraries").await;
    assert_eq!(libraries[0]["latestScan"]["cancellationRequested"], true);
    fs::remove_dir_all(base).unwrap();
}
