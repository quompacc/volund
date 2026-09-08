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

use support::{authenticated_router, test_database};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn unique_fixture() -> (String, String, PathBuf) {
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    (
        format!("commit_{unique}"),
        format!("commit-model-{unique}"),
        std::env::temp_dir().join(format!("volund-commit-{unique}")),
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
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

async fn get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

async fn post_bytes(router: &axum::Router, uri: &str, body: Vec<u8>) -> (StatusCode, Value) {
    let length = body.len();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, length)
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
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

async fn reviewed_import(
    router: &axum::Router,
    model_name: &str,
    library_root_id: &str,
    entries: &[(&str, Vec<u8>)],
) -> Value {
    let collection_name = format!("{model_name} Gruppe");
    let (status, collection) = post_json(
        router,
        "/api/v1/collections",
        serde_json::json!({ "name": collection_name, "description": "Importgruppe" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let manifest: Vec<Value> = entries
        .iter()
        .map(|(path, body)| serde_json::json!({ "path": path, "byteSize": body.len() }))
        .collect();
    let (status, draft) = post_json(
        router,
        "/api/v1/imports/preview",
        serde_json::json!({ "sourceName": model_name, "entries": manifest }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let draft_id = draft["id"].as_str().expect("draft ID");
    let (status, _) = post_json(
        router,
        &format!("/api/v1/imports/{draft_id}/metadata"),
        serde_json::json!({
            "modelName": model_name, "kind": "assembly", "libraryRootId": library_root_id,
            "description": "Import test",
            "authorName": "VÖLUND Test", "tags": ["Import", "CAD"],
            "collectionIds": [collection["id"]]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for (index, (_, body)) in entries.iter().enumerate() {
        let item_id = draft["items"][index]["id"].as_str().expect("item ID");
        let (status, _) = post_bytes(
            router,
            &format!("/api/v1/imports/{draft_id}/items/{item_id}/content"),
            body.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, review) = post_json(
        router,
        &format!("/api/v1/imports/{draft_id}/review"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    review
}

#[tokio::test]
async fn confirmation_publishes_all_reviewed_files_and_model_metadata() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, model_name, fixture) = unique_fixture();
    let library = fixture.join("library");
    fs::create_dir_all(library.join("Test/Baugruppen")).expect("create library");
    fs::write(library.join("Test/Baugruppen/main.step"), b"existing-step")
        .expect("write existing STEP");
    scanner::register_root(&pool, &root_key, "Commit Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan root");
    let library_root_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots WHERE root_key = $1")
            .bind(&root_key)
            .fetch_one(&*pool)
            .await
            .expect("read library ID");
    fs::create_dir_all(fixture.join("web")).expect("create web root");
    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), fixture.join("derived"), fixture.join("web")),
    )
    .await;
    let entries = [
        ("Package/CAD/main.step", b"existing-step".to_vec()),
        ("Package/STLs/panel.stl", b"mesh-body".to_vec()),
        ("Package/docs/manual.pdf", b"manual-body".to_vec()),
    ];
    let review = reviewed_import(&router, &model_name, &library_root_id, &entries).await;
    assert_eq!(review["createFiles"], 2);
    assert_eq!(review["relocateFiles"], 1);
    assert_eq!(review["items"].as_array().expect("review items").len(), 3);
    let draft_id = review["draftId"].as_str().expect("draft ID");
    let (status, committed) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/commit"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(committed["status"], "committed");
    assert_eq!(committed["totalFiles"], 3);
    assert_eq!(committed["createdFiles"], 2);
    assert_eq!(committed["relocatedFiles"], 1);
    for item in review["items"].as_array().expect("review items") {
        assert!(
            library
                .join(item["targetPath"].as_str().expect("target path"))
                .is_file()
        );
    }
    assert!(!library.join("Test/Baugruppen/main.step").exists());
    assert!(!fixture.join("incoming").join(draft_id).exists());
    let linked: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.model_source_files link JOIN volund.models model \
         ON model.id = link.model_id WHERE model.public_id::text = $1",
    )
    .bind(committed["modelId"].as_str().expect("model ID"))
    .fetch_one(&*pool)
    .await
    .expect("count linked files");
    assert_eq!(linked, 3);
    let (status, models) = get_json(&router, "/api/v1/models").await;
    assert_eq!(status, StatusCode::OK);
    let imported = models
        .as_array()
        .expect("model array")
        .iter()
        .find(|model| model["id"] == committed["modelId"])
        .expect("imported model in catalog");
    assert_eq!(imported["tags"], serde_json::json!(["CAD", "Import"]));
    assert_eq!(
        imported["collections"],
        serde_json::json!([format!("{model_name} Gruppe")])
    );
    let (status, collections) = get_json(&router, "/api/v1/collections").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        collections["items"]
            .as_array()
            .expect("collections")
            .iter()
            .any(|collection| {
                collection["name"] == format!("{model_name} Gruppe")
                    && collection["modelCount"] == 1
            })
    );
    fs::remove_dir_all(fixture).expect("remove fixture");
}

#[tokio::test]
async fn confirmation_refuses_new_target_conflicts_without_losing_staged_data() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, model_name, fixture) = unique_fixture();
    let library = fixture.join("library");
    fs::create_dir_all(library.join("Test/Baugruppen")).expect("create library");
    fs::write(library.join("Test/Baugruppen/main.step"), b"existing-step")
        .expect("write existing STEP");
    scanner::register_root(&pool, &root_key, "Rollback Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan root");
    let library_root_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots WHERE root_key = $1")
            .bind(&root_key)
            .fetch_one(&*pool)
            .await
            .expect("read library ID");
    fs::create_dir_all(fixture.join("web")).expect("create web root");
    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), fixture.join("derived"), fixture.join("web")),
    )
    .await;
    let entries = [
        ("Package/CAD/main.step", b"existing-step".to_vec()),
        ("Package/CAD/new.step", b"new-step".to_vec()),
    ];
    let review = reviewed_import(&router, &model_name, &library_root_id, &entries).await;
    let draft_id = review["draftId"].as_str().expect("draft ID");
    let create_item = review["items"]
        .as_array()
        .expect("review items")
        .iter()
        .find(|item| item["action"] == "create")
        .expect("create action");
    let target = library.join(create_item["targetPath"].as_str().expect("target path"));
    fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
    fs::write(&target, b"late-conflict").expect("write late conflict");
    let (status, error) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/commit"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "conflict");
    assert_eq!(fs::read(&target).expect("read conflict"), b"late-conflict");
    assert!(library.join("Test/Baugruppen/main.step").exists());
    assert!(fixture.join("incoming").join(draft_id).exists());
    let status: String =
        sqlx::query_scalar("SELECT status FROM volund.import_drafts WHERE public_id::text = $1")
            .bind(draft_id)
            .fetch_one(&*pool)
            .await
            .expect("read draft status");
    assert_eq!(status, "failed");
    fs::remove_dir_all(fixture).expect("remove fixture");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn extension_adds_files_without_replacing_model_metadata_or_relationships() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, model_name, fixture) = unique_fixture();
    let library = fixture.join("library");
    fs::create_dir_all(&library).expect("create library");
    scanner::register_root(&pool, &root_key, "Extension Test", &library)
        .await
        .expect("register root");
    let library_root_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots WHERE root_key=$1")
            .bind(&root_key)
            .fetch_one(&*pool)
            .await
            .expect("read library ID");
    fs::create_dir_all(fixture.join("web")).expect("create web root");
    let router = authenticated_router(
        &pool,
        api::router_with_roots(pool.clone(), fixture.join("derived"), fixture.join("web")),
    )
    .await;

    let initial = [("Package/CAD/main.step", b"initial-step".to_vec())];
    let review = reviewed_import(&router, &model_name, &library_root_id, &initial).await;
    let (status, committed) = post_json(
        &router,
        &format!(
            "/api/v1/imports/{}/commit",
            review["draftId"].as_str().expect("initial draft ID")
        ),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let model_id = committed["modelId"].as_str().expect("model ID");
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'name',model.name,'description',model.description,'kind',model.kind, \
         'authorId',model.author_id,'licenseKind',model.license_kind,'licenseValue',model.license_value, \
         'thumbnailKind',model.thumbnail_kind,'thumbnailSource',model.thumbnail_source_file_id, \
         'thumbnailArtifact',model.thumbnail_artifact_id, \
         'tags',COALESCE((SELECT jsonb_agg(link.tag_id ORDER BY link.tag_id) FROM volund.model_tags link WHERE link.model_id=model.id),'[]'::jsonb), \
         'collections',COALESCE((SELECT jsonb_agg(link.collection_id ORDER BY link.collection_id) FROM volund.collection_models link WHERE link.model_id=model.id),'[]'::jsonb), \
         'primary',COALESCE((SELECT jsonb_agg(jsonb_build_array(link.source_file_id,link.role,link.is_primary,link.ordinal) ORDER BY link.source_file_id) FROM volund.model_source_files link WHERE link.model_id=model.id AND link.is_primary),'[]'::jsonb)) \
         FROM volund.models model WHERE model.public_id::text=$1",
    )
    .bind(model_id)
    .fetch_one(&*pool)
    .await
    .expect("snapshot model relationships");
    let before_links: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.model_source_files link JOIN volund.models model ON model.id=link.model_id WHERE model.public_id::text=$1",
    )
    .bind(model_id)
    .fetch_one(&*pool)
    .await
    .expect("count initial links");
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM volund.models WHERE public_id::text=$1")
            .bind(model_id)
            .fetch_one(&*pool)
            .await
            .expect("read model revision");

    let body = b"new-extension-mesh".to_vec();
    let (status, draft) = post_json(
        &router,
        "/api/v1/imports/preview",
        serde_json::json!({
            "sourceName": "extension",
            "entries": [{"path": "Extension/STLs/new-part.stl", "byteSize": body.len()}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let draft_id = draft["id"].as_str().expect("extension draft ID");
    let (status, _) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/metadata"),
        serde_json::json!({
            "modelName": "must not replace the name", "kind": "part",
            "libraryRootId": library_root_id, "description": "must not replace description",
            "authorName": "Must Not Be Created", "tags": ["MustNotReplace"], "collectionIds": [],
            "targetAction": "extend", "targetModelId": model_id,
            "expectedModelRevision": revision, "licenseKind": "custom", "licenseValue": "must-not-replace"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let item_id = draft["items"][0]["id"].as_str().expect("extension item ID");
    let (status, _) = post_bytes(
        &router,
        &format!("/api/v1/imports/{draft_id}/items/{item_id}/content"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, review) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/review"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["modelAction"], "extend");
    let (status, extension) = post_json(
        &router,
        &format!("/api/v1/imports/{draft_id}/commit"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(extension["modelId"], model_id);

    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'name',model.name,'description',model.description,'kind',model.kind, \
         'authorId',model.author_id,'licenseKind',model.license_kind,'licenseValue',model.license_value, \
         'thumbnailKind',model.thumbnail_kind,'thumbnailSource',model.thumbnail_source_file_id, \
         'thumbnailArtifact',model.thumbnail_artifact_id, \
         'tags',COALESCE((SELECT jsonb_agg(link.tag_id ORDER BY link.tag_id) FROM volund.model_tags link WHERE link.model_id=model.id),'[]'::jsonb), \
         'collections',COALESCE((SELECT jsonb_agg(link.collection_id ORDER BY link.collection_id) FROM volund.collection_models link WHERE link.model_id=model.id),'[]'::jsonb), \
         'primary',COALESCE((SELECT jsonb_agg(jsonb_build_array(link.source_file_id,link.role,link.is_primary,link.ordinal) ORDER BY link.source_file_id) FROM volund.model_source_files link WHERE link.model_id=model.id AND link.is_primary),'[]'::jsonb)) \
         FROM volund.models model WHERE model.public_id::text=$1",
    )
    .bind(model_id)
    .fetch_one(&*pool)
    .await
    .expect("snapshot extended model relationships");
    assert_eq!(after, before);
    let after_links: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM volund.model_source_files link JOIN volund.models model ON model.id=link.model_id WHERE model.public_id::text=$1",
    )
    .bind(model_id)
    .fetch_one(&*pool)
    .await
    .expect("count extended links");
    assert_eq!(after_links, before_links + 1);
    let after_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM volund.models WHERE public_id::text=$1")
            .bind(model_id)
            .fetch_one(&*pool)
            .await
            .expect("read extended revision");
    assert_eq!(after_revision, revision + 1);
    fs::remove_dir_all(fixture).expect("remove fixture");
}
