#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;

use support::{TestDatabase, authenticated_router, get_json, test_database};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn unique_fixture() -> (String, PathBuf) {
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    (
        format!("api_{unique}"),
        PathBuf::from(format!("/tmp/volund-api-{unique}")),
    )
}

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("route request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("parse JSON response"),
    )
}

async fn post_bytes(
    router: &axum::Router,
    uri: &str,
    body: Vec<u8>,
    declared_length: usize,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, declared_length)
                .body(Body::from(body))
                .expect("build byte request"),
        )
        .await
        .expect("route byte request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read byte response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("parse byte response JSON"),
    )
}

async fn get_text(router: &axum::Router, uri: &str) -> (StatusCode, String, axum::http::HeaderMap) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body");
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("UTF-8 body"),
        headers,
    )
}

async fn nested_catalog_fixture() -> Option<(axum::Router, TestDatabase, String, PathBuf)> {
    let pool = test_database().await?;
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(root.join("printers/archive")).expect("create printer folders");
    fs::create_dir_all(root.join("meshes")).expect("create mesh folder");
    fs::write(root.join("root.step"), b"root").expect("write root file");
    fs::write(root.join("printers/voron.step"), b"voron assembly").expect("write printer");
    fs::write(root.join("printers/archive/legacy.stl"), b"legacy mesh")
        .expect("write archive file");
    fs::write(root.join("meshes/frame.3mf"), b"frame mesh").expect("write mesh");
    scanner::register_root(&pool, &root_key, "Nested Catalog", &root)
        .await
        .expect("register nested root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan nested root");
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    Some((router, pool, root_key, root))
}

#[tokio::test]
async fn read_only_api_lists_catalog_state_with_bounded_pagination() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(&root).expect("create API fixture root");
    fs::write(root.join("part.step"), b"step fixture").expect("write STEP fixture");
    fs::write(root.join("part-copy.stp"), b"step fixture").expect("write duplicate fixture");
    fs::write(root.join("mesh.stl"), b"stl fixture").expect("write STL fixture");
    scanner::register_root(&pool, &root_key, "API Test", &root)
        .await
        .expect("register API root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("initial API fixture scan");
    fs::remove_file(root.join("mesh.stl")).expect("remove fixture path");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("missing-path API fixture scan");
    let router = authenticated_router(&pool, api::router(pool.clone())).await;

    let (status, health) = get_json(&router, "/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));

    let (status, roots) = get_json(&router, "/api/v1/roots").await;
    assert_eq!(status, StatusCode::OK);
    let root_json = roots
        .as_array()
        .expect("root array")
        .iter()
        .find(|item| item["key"] == root_key)
        .expect("registered root response");
    assert_eq!(root_json["fileCount"], 3);
    assert_eq!(root_json["missingFileCount"], 1);
    assert!(root_json.get("filesystemPath").is_none());

    let (status, current) =
        get_json(&router, &format!("/api/v1/roots/{root_key}/files?limit=1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["total"], 2);
    assert_eq!(current["items"].as_array().expect("file array").len(), 1);
    assert_eq!(current["items"][0]["path"], "part-copy.stp");

    let (status, all_files) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?includeMissing=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all_files["total"], 3);

    let sha256 = current["items"][0]["sha256"]
        .as_str()
        .expect("content hash");
    let (status, content) = get_json(&router, &format!("/api/v1/content/{sha256}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content["sourceCount"], 2);
    assert_eq!(content["availableSourceCount"], 2);
    let (status, invalid_hash) = get_json(&router, "/api/v1/content/not-a-hash").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_hash["error"]["code"], "bad_request");

    let (status, scans) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/scans?limit=1&offset=1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scans["total"], 2);
    assert_eq!(scans["limit"], 1);
    assert_eq!(scans["offset"], 1);

    let (status, missing_root) = get_json(&router, "/api/v1/roots/unknown/files").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing_root["error"]["code"], "not_found");
    let (status, invalid_page) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?limit=201"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_page["error"]["code"], "bad_request");
    let (status, fallback) = get_json(&router, "/api/v1/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(fallback["error"]["code"], "not_found");

    fs::remove_dir_all(root).expect("remove API fixture root");
}

#[tokio::test]
async fn static_web_application_is_isolated_from_api_fallback() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (_, root) = unique_fixture();
    let web = root.join("web");
    fs::create_dir_all(web.join("assets")).expect("create web fixture");
    fs::write(
        web.join("index.html"),
        "<!doctype html><title>VÖLUND</title>",
    )
    .expect("write web index");
    fs::write(web.join("assets/app.js"), "console.log('volund')").expect("write web asset");
    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), root.join("derived"), web),
    )
    .await;

    let (status, index, headers) = get_text(&router, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(index.contains("VÖLUND"));
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
    assert!(
        headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("CSP")
            .contains("default-src 'self'")
    );
    let (status, script, _) = get_text(&router, "/assets/app.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(script, "console.log('volund')");
    let (status, spa, _) = get_text(&router, "/library/cad").await;
    assert_eq!(status, StatusCode::OK);
    assert!(spa.contains("VÖLUND"));
    let (status, fallback) = get_json(&router, "/api/v1/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(fallback["error"]["code"], "not_found");

    fs::remove_dir_all(root).expect("remove web fixture root");
}

#[tokio::test]
async fn catalog_navigates_folders_and_searches_recursively() {
    let Some((router, _database, root_key, root)) = nested_catalog_fixture().await else {
        return;
    };
    let (status, root_folders) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/folders?limit=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(root_folders["total"], 2);
    assert_eq!(root_folders["items"][0]["path"], "meshes");
    assert_eq!(root_folders["items"][1]["path"], "printers");
    assert_eq!(root_folders["items"][1]["fileCount"], 2);

    let (status, root_files) =
        get_json(&router, &format!("/api/v1/roots/{root_key}/files?limit=20")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(root_files["total"], 1);
    assert_eq!(root_files["items"][0]["path"], "root.step");

    let (status, printer_files) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?directory=printers&limit=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(printer_files["total"], 1);
    assert_eq!(printer_files["items"][0]["path"], "printers/voron.step");
    let (status, printer_folders) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/folders?directory=printers&limit=20"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(printer_folders["items"][0]["path"], "printers/archive");

    let (status, search) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?directory=printers&q=legacy&format=stl&sort=size&direction=desc"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(search["total"], 1);
    assert_eq!(search["items"][0]["path"], "printers/archive/legacy.stl");
    let (status, invalid) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?directory=../secret"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["error"]["code"], "bad_request");
    let (status, invalid) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?sort=random"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["error"]["code"], "bad_request");

    fs::remove_dir_all(root).expect("remove nested fixture root");
}

#[tokio::test]
async fn managed_move_preserves_file_identity_and_refuses_overwrites() {
    let Some((router, pool, root_key, root)) = nested_catalog_fixture().await else {
        return;
    };
    let (_, root_files) =
        get_json(&router, &format!("/api/v1/roots/{root_key}/files?limit=20")).await;
    let file_id = root_files["items"][0]["id"]
        .as_str()
        .expect("file public ID")
        .to_owned();
    let model_slug = root_key.replace('_', "-");
    let (status, model) = post_json(
        &router,
        "/api/v1/models",
        serde_json::json!({
            "name": "Root Assembly",
            "slug": model_slug,
            "kind": "assembly",
            "primaryFileId": file_id,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(model["fileCount"], 1);
    let model_id = model["id"].as_str().expect("model ID").to_owned();

    let (status, moved) = post_json(
        &router,
        &format!("/api/v1/files/{file_id}/move"),
        serde_json::json!({ "destinationDirectory": "printers" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["id"], file_id);
    assert_eq!(moved["previousPath"], "root.step");
    assert_eq!(moved["path"], "printers/root.step");
    assert!(!root.join("root.step").exists());
    assert!(root.join("printers/root.step").exists());

    let (_, printer_files) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/files?directory=printers&limit=20"),
    )
    .await;
    let moved_file = printer_files["items"]
        .as_array()
        .expect("file array")
        .iter()
        .find(|item| item["id"] == file_id)
        .expect("moved file");
    assert_eq!(moved_file["path"], "printers/root.step");
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.source_file_moves moves \
         JOIN volund.source_files source ON source.id = moves.source_file_id \
         WHERE source.public_id::text = $1",
    )
    .bind(&file_id)
    .fetch_one(&*pool)
    .await
    .expect("count move audit entries");
    assert_eq!(audit_count, 1);
    let (status, models) = get_json(&router, "/api/v1/models").await;
    assert_eq!(status, StatusCode::OK);
    let stable_model = models
        .as_array()
        .expect("model array")
        .iter()
        .find(|item| item["id"] == model_id)
        .expect("stable model relation");
    assert_eq!(stable_model["fileCount"], 1);
    assert_eq!(stable_model["formats"][0], "step");
    let (status, model_files) =
        get_json(&router, &format!("/api/v1/models/{model_id}/files")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(model_files["total"], 1);
    assert_eq!(model_files["items"][0]["id"], file_id);
    assert_eq!(model_files["items"][0]["path"], "printers/root.step");
    assert_eq!(model_files["items"][0]["role"], "master-cad");
    assert_eq!(model_files["items"][0]["primary"], true);
    assert_eq!(model_files["items"][0]["rootKey"], root_key);

    fs::write(root.join("meshes/root.step"), b"conflict").expect("write conflict target");
    let (status, conflict) = post_json(
        &router,
        &format!("/api/v1/files/{file_id}/move"),
        serde_json::json!({ "destinationDirectory": "meshes" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "conflict");
    assert!(root.join("printers/root.step").exists());

    fs::remove_dir_all(root).expect("remove move fixture root");
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One end-to-end import lifecycle with shared state.
async fn import_preview_is_classified_persisted_and_path_safe() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    let library = root.join("library");
    fs::create_dir_all(library.join("Testbibliothek/Baugruppen"))
        .expect("create import library root");
    fs::write(
        library.join("Testbibliothek/Baugruppen/main.step"),
        vec![b'x'; 500],
    )
    .expect("write existing primary CAD");
    scanner::register_root(&pool, &root_key, "Import Review", &library)
        .await
        .expect("register import review root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan import review root");
    let library_root_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots WHERE root_key = $1")
            .bind(&root_key)
            .fetch_one(&*pool)
            .await
            .expect("read import library ID");
    fs::create_dir_all(root.join("web")).expect("create import web root");
    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), root.join("derived"), root.join("web")),
    )
    .await;
    let (status, draft) = post_json(
        &router,
        "/api/v1/imports/preview",
        serde_json::json!({
            "sourceName": "VÖLUND Projekt.zip",
            "entries": [
                { "path": "Voron/CAD/main.step", "byteSize": 500 },
                { "path": "Voron/STLs/panel.stl", "byteSize": 200 },
                { "path": "Voron/docs/manual.pdf", "byteSize": 100 }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["suggestedModelName"], "VÖLUND Projekt");
    assert_eq!(draft["suggestedSlug"], "voelund-projekt");
    assert_eq!(draft["totalFiles"], 3);
    assert_eq!(draft["items"][0]["category"], "cad");
    assert_eq!(draft["items"][0]["isPrimaryCandidate"], true);
    let draft_id = draft["id"].as_str().expect("draft ID");
    let stored_items: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.import_draft_items item \
         JOIN volund.import_drafts draft ON draft.id = item.import_draft_id \
         WHERE draft.public_id::text = $1",
    )
    .bind(draft_id)
    .fetch_one(&*pool)
    .await
    .expect("count draft items");
    assert_eq!(stored_items, 3);

    let (status, configured) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/metadata"),
        serde_json::json!({
            "modelName": " VORON 2.4 ",
            "kind": "assembly",
            "libraryRootId": library_root_id,
            "description": "CoreXY Drucker",
            "authorName": "Voron Design",
            "tags": ["Voron", "voron", "CoreXY"]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(configured["modelName"], "VORON 2.4");
    assert_eq!(configured["slug"], "voron-2-4");
    assert_eq!(configured["libraryRootId"], library_root_id);
    assert_eq!(configured["tags"], serde_json::json!(["Voron", "CoreXY"]));
    assert_eq!(configured["readyForUpload"], true);
    let stored_kind: Option<String> = sqlx::query_scalar(
        "SELECT model_kind FROM volund.import_drafts WHERE public_id::text = $1",
    )
    .bind(draft_id)
    .fetch_one(&*pool)
    .await
    .expect("read configured draft");
    assert_eq!(stored_kind.as_deref(), Some("assembly"));

    let item_id = draft["items"][0]["id"].as_str().expect("draft item ID");
    let upload_uri = format!("/api/v1/imports/{draft_id}/items/{item_id}/content");
    let (status, wrong_length) = post_bytes(&router, &upload_uri, vec![b'x'; 500], 499).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(wrong_length["error"]["code"], "bad_request");

    let (status, uploaded) = post_bytes(&router, &upload_uri, vec![b'x'; 500], 500).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(uploaded["byteSize"], 500);
    assert_eq!(uploaded["status"], "uploaded");
    assert_eq!(uploaded["alreadyUploaded"], false);
    assert_eq!(uploaded["sha256"].as_str().expect("SHA-256").len(), 64);
    let staged = root
        .join("incoming")
        .join(draft_id)
        .join(format!("{item_id}.bin"));
    assert_eq!(fs::metadata(staged).expect("staged upload").len(), 500);

    let (status, repeated) = post_bytes(&router, &upload_uri, vec![b'x'; 500], 500).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["alreadyUploaded"], true);

    for (index, size, byte) in [(1_usize, 200_usize, b'm'), (2, 100, b'd')] {
        let next_item_id = draft["items"][index]["id"].as_str().expect("next item ID");
        let next_uri = format!("/api/v1/imports/{draft_id}/items/{next_item_id}/content");
        let (status, _) = post_bytes(&router, &next_uri, vec![byte; size], size).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, latest) = get_json(&router, "/api/v1/imports/latest-uploaded").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest["id"], draft_id);
    let (status, review) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/review"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["rootKey"], root_key);
    assert_eq!(
        review["baseDirectory"],
        "Testbibliothek/Baugruppen/voron-2-4"
    );
    assert_eq!(review["createFiles"], 2);
    assert_eq!(review["relocateFiles"], 1);
    assert_eq!(review["reuseFiles"], 0);
    assert_eq!(review["conflicts"], 0);
    assert_eq!(review["savedBytes"], 500);
    assert_eq!(review["modelAction"], "create");
    assert_eq!(review["items"][0]["action"], "relocate");
    assert_eq!(
        review["items"][0]["existingPath"],
        "Testbibliothek/Baugruppen/main.step"
    );

    let (status, missing) = post_json(
        &router,
        "/api/v1/imports/00000000-0000-0000-0000-000000000000/metadata",
        serde_json::json!({
            "modelName": "Missing", "kind": "project", "description": "",
            "libraryRootId": library_root_id, "authorName": null, "tags": []
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "not_found");

    let (status, invalid) = post_json(
        &router,
        "/api/v1/imports/preview",
        serde_json::json!({
            "sourceName": "unsafe.zip",
            "entries": [{ "path": "../escape.step", "byteSize": 1 }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["error"]["code"], "bad_request");
    fs::remove_dir_all(root).expect("remove import fixture root");
}
