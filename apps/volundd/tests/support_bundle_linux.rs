#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use volundd::operational_log::{self, Event, Severity};
use volundd::{api, identity::IdentityConfig};

mod support;
use support::{authenticated_router, test_database};

#[tokio::test]
#[allow(clippy::too_many_lines)] // One archive lifecycle must retain shared session and fixture state.
async fn support_bundle_is_bounded_redacted_audited_and_cleaned() {
    let Some(pool) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-support-{suffix}"));
    let derived = root.join("derived");
    let web = root.join("web");
    fs::create_dir_all(&derived).expect("create derived fixture");
    fs::create_dir_all(&web).expect("create web fixture");
    fs::write(web.join("index.html"), b"support fixture").expect("write web fixture");

    operational_log::record(
        &pool,
        &Event {
            severity: Severity::Error,
            event: "fixture.failure",
            component: "daemon",
            code: "fixture_failed",
            message: "Authorization: Bearer forbidden-token /srv/private/original.step",
            request_id: Some("req-fixture"),
            job_id: None,
            run_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("record redacted fixture");

    let public = api::router_with_identity(
        (*pool).clone(),
        derived.clone(),
        web.clone(),
        IdentityConfig::default(),
    );
    let response = public
        .clone()
        .oneshot(
            Request::post("/api/v1/operations/support-bundle")
                .body(Body::empty())
                .expect("anonymous request"),
        )
        .await
        .expect("anonymous response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let router = authenticated_router(&pool, public).await;
    let cross_origin = router
        .clone()
        .oneshot(
            Request::post("/api/v1/operations/support-bundle")
                .header(header::HOST, "volund.test")
                .header(header::ORIGIN, "http://attacker.test")
                .body(Body::empty())
                .expect("cross-origin request"),
        )
        .await
        .expect("cross-origin response");
    assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);
    let origin_denials: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action='request.mutation' AND outcome='denied' \
         AND metadata->>'code'='origin_mismatch'",
    )
    .fetch_one(&*pool)
    .await
    .expect("origin audit");
    assert_eq!(origin_denials, 1);

    let response = router
        .clone()
        .oneshot(
            Request::post("/api/v1/operations/support-bundle")
                .header(header::HOST, "volund.test")
                .header(header::ORIGIN, "http://volund.test")
                .body(Body::empty())
                .expect("support request"),
        )
        .await
        .expect("support response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/x-tar"
    );
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("disposition")
            .starts_with("attachment; filename=\"volund-support-")
    );
    let expected_hash = response.headers()["x-content-sha256"]
        .to_str()
        .expect("hash")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read archive");
    assert!(bytes.len() < 4 * 1024 * 1024);
    assert_eq!(hex::encode(Sha256::digest(&bytes)), expected_hash);

    let members = parse_tar(&bytes);
    assert_eq!(members.len(), 8);
    let manifest: Value =
        serde_json::from_slice(&members["manifest.json"]).expect("parse support manifest");
    for member in manifest["members"].as_array().expect("manifest members") {
        let name = member["name"].as_str().expect("member name");
        let content = &members[name];
        assert_eq!(member["bytes"], i64::try_from(content.len()).unwrap());
        assert_eq!(member["sha256"], hex::encode(Sha256::digest(content)));
    }
    let combined = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    for forbidden in [
        "forbidden-token",
        "bearer",
        "/srv/private",
        "postgresql://",
        "private key",
        "volund_session",
    ] {
        assert!(!combined.contains(forbidden), "archive leaked {forbidden}");
    }
    assert!(combined.contains("diagnostic redacted"));
    let support_root = root.join("support");
    assert_eq!(
        fs::read_dir(&support_root).expect("support root").count(),
        0
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action='support-bundle.create' AND outcome='success'",
    )
    .fetch_one(&*pool)
    .await
    .expect("support audit");
    assert_eq!(audits, 1);

    sqlx::query("UPDATE volund.users SET role='viewer'")
        .execute(&*pool)
        .await
        .expect("set viewer");
    let denied = router
        .oneshot(
            Request::post("/api/v1/operations/support-bundle")
                .body(Body::empty())
                .expect("viewer request"),
        )
        .await
        .expect("viewer response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let capability_denials: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action='support-bundle.create' AND outcome='denied' \
         AND metadata->>'code'='capability_denied'",
    )
    .fetch_one(&*pool)
    .await
    .expect("capability audit");
    assert_eq!(capability_denials, 1);
    fs::remove_dir_all(root).expect("remove support fixture");
}

fn parse_tar(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut offset = 0;
    let mut members = HashMap::new();
    while offset + 512 <= bytes.len() && bytes[offset..offset + 512].iter().any(|byte| *byte != 0) {
        let header = &bytes[offset..offset + 512];
        let name_end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        let name = std::str::from_utf8(&header[..name_end])
            .expect("tar name")
            .to_owned();
        let size_text = std::str::from_utf8(&header[124..136])
            .expect("tar size")
            .trim_matches(char::from(0))
            .trim_start_matches('0');
        let size = if size_text.is_empty() {
            0
        } else {
            usize::from_str_radix(size_text, 8).expect("octal size")
        };
        offset += 512;
        members.insert(name, bytes[offset..offset + size].to_vec());
        offset += size.div_ceil(512) * 512;
    }
    members
}
