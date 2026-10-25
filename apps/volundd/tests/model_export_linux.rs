#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use volundd::{api, identity::IdentityConfig, scanner};

mod support;
use support::{assert_matches_schema, authenticated_router, test_database};

const SCHEMA: &str = include_str!("../../../contracts/model-export-v1.schema.json");

const MAIN_STEP: &[u8] = b"ISO-10303-21; export fixture";
const COVER_PNG: &[u8] = b"raster fixture bytes";

struct Fixture {
    root: PathBuf,
    library: PathBuf,
    model_id: String,
}

async fn fixture(pool: &sqlx::PgPool) -> Fixture {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-model-export-{suffix}"));
    let library = root.join("library");
    fs::create_dir_all(library.join("Projekte/frame/CAD")).expect("create library fixture");
    fs::create_dir_all(library.join("Projekte/frame/Bilder")).expect("create image folder");
    fs::create_dir_all(root.join("derived")).expect("create derived fixture");
    fs::write(library.join("Projekte/frame/CAD/main.step"), MAIN_STEP).expect("write STEP");
    fs::write(library.join("Projekte/frame/Bilder/cover.png"), COVER_PNG).expect("write PNG");
    fs::write(library.join("Projekte/frame/gone.stl"), b"solid gone").expect("write STL");
    scanner::register_root(pool, "export_test", "Export Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(pool, "export_test", false)
        .await
        .expect("scan fixture");
    let model_id: String = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind,description) \
         VALUES ('frame-export','Frame export','assembly','Exit package fixture') RETURNING public_id::text",
    )
    .fetch_one(pool)
    .await
    .expect("create model");
    sqlx::query(
        "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal) \
         SELECT model.id, source.id, \
           CASE WHEN source.relative_path LIKE '%.step' THEN 'master-cad' \
                WHEN source.relative_path LIKE '%.png' THEN 'image' ELSE 'printable-mesh' END, \
           source.relative_path LIKE '%.step', row_number() OVER (ORDER BY source.relative_path) \
         FROM volund.models model, volund.source_files source \
         WHERE model.public_id::text=$1",
    )
    .bind(&model_id)
    .execute(pool)
    .await
    .expect("link model files");
    // Remove one original after cataloguing: the package must report it instead of failing.
    fs::remove_file(library.join("Projekte/frame/gone.stl")).expect("remove STL");
    Fixture {
        root,
        library,
        model_id,
    }
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("export request");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("read export body")
        .to_vec();
    (status, headers, bytes)
}

fn member(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("archive member {name}"))
        .read_to_end(&mut bytes)
        .expect("read archive member");
    bytes
}

fn staging_entries(support: &Path) -> usize {
    fs::read_dir(support).map_or(0, Iterator::count)
}

async fn audit_rows(pool: &sqlx::PgPool, model_id: &str, outcome: &str) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT metadata FROM volund.security_audit_events \
         WHERE action='model.export' AND target_public_id::text=$1 AND outcome=$2",
    )
    .bind(model_id)
    .bind(outcome)
    .fetch_all(pool)
    .await
    .expect("read export audit")
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One package lifecycle shares fixture, router and audit state.
async fn model_export_packages_verified_originals_with_manifest_and_audit() {
    let Some(pool) = test_database().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let router = authenticated_router(
        &pool,
        api::router_with_identity(
            (*pool).clone(),
            fixture.root.join("derived"),
            fixture.root.join("web"),
            IdentityConfig::default(),
        ),
    )
    .await;
    let support = fixture.root.join("support");
    let uri = format!("/api/v1/models/{}/export", fixture.model_id);

    let (status, headers, body) = get(&router, &uri).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(headers["x-volund-export-format"], "volund-model-export-v1");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("disposition")
            .contains("frame-export-volund-export.zip")
    );
    assert_eq!(
        staging_entries(&support),
        0,
        "staging must not keep the archive"
    );

    let mut archive = zip::ZipArchive::new(Cursor::new(body)).expect("valid zip");
    assert_eq!(
        member(
            &mut archive,
            "originals/export_test/Projekte/frame/CAD/main.step"
        ),
        MAIN_STEP
    );
    assert_eq!(
        member(
            &mut archive,
            "originals/export_test/Projekte/frame/Bilder/cover.png"
        ),
        COVER_PNG
    );
    assert!(
        archive
            .by_name("originals/export_test/Projekte/frame/gone.stl")
            .is_err()
    );
    let checksums = String::from_utf8(member(&mut archive, "SHA256SUMS")).expect("UTF-8 sums");
    let step_sha = format!("{:x}", Sha256::digest(MAIN_STEP));
    assert!(checksums.contains(&format!(
        "{step_sha}  originals/export_test/Projekte/frame/CAD/main.step"
    )));
    assert_eq!(checksums.lines().count(), 2);

    let manifest: Value =
        serde_json::from_slice(&member(&mut archive, "manifest.json")).expect("manifest JSON");
    let schema: Value = serde_json::from_str(SCHEMA).expect("schema JSON");
    assert_matches_schema(&schema, &manifest, "manifest");
    assert_eq!(manifest["format"], "volund-model-export-v1");
    assert_eq!(manifest["formatVersion"], 1);
    assert_eq!(manifest["model"]["id"], fixture.model_id.as_str());
    assert_eq!(manifest["model"]["description"], "Exit package fixture");
    let files = manifest["files"].as_array().expect("manifest files");
    assert_eq!(files.len(), 3);
    let by_path = |suffix: &str| {
        files
            .iter()
            .find(|file| {
                file["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(suffix))
            })
            .unwrap_or_else(|| panic!("manifest entry {suffix}"))
    };
    assert_eq!(by_path("main.step")["included"], true);
    assert_eq!(by_path("main.step")["role"], "master-cad");
    assert_eq!(by_path("main.step")["sha256"], step_sha.as_str());
    assert_eq!(by_path("gone.stl")["included"], false);
    assert_eq!(by_path("gone.stl")["exclusionReason"], "missing");
    assert_eq!(by_path("gone.stl")["archivePath"], Value::Null);
    let serialized = manifest.to_string();
    assert!(
        !serialized.contains(fixture.library.to_str().expect("library path")),
        "manifest must not reveal host library paths"
    );

    let success = audit_rows(&pool, &fixture.model_id, "success").await;
    assert_eq!(success.len(), 1);
    assert_eq!(success[0]["includedFiles"], 2);
    assert_eq!(success[0]["excludedFiles"], 1);

    // Same size, different bytes: the catalog hash no longer matches.
    fs::write(
        fixture.library.join("Projekte/frame/CAD/main.step"),
        b"ISO-10303-21; tampered fixture",
    )
    .expect("tamper STEP");
    let (status, _, body) = get(&router, &uri).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("model_export_source_changed"));
    assert_eq!(staging_entries(&support), 0, "failed exports must clean up");
    let failure = audit_rows(&pool, &fixture.model_id, "failure").await;
    assert_eq!(failure.len(), 1);
    assert_eq!(failure[0]["code"], "model_export_source_changed");

    let (status, _, _) = get(
        &router,
        "/api/v1/models/00000000-0000-4000-8000-000000000000/export",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    fs::remove_dir_all(fixture.root).expect("remove export fixture");
}
