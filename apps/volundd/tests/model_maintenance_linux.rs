#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;

use support::{authenticated_router, get_json, test_database};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

fn unique_fixture() -> (String, PathBuf) {
    let unique =
        u64::from(std::process::id()) * 1_000_000 + UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    (
        format!("maintenance_{unique}"),
        std::env::temp_dir().join(format!("volund-maintenance-{unique}")),
    )
}

async fn mutation_json(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse JSON response")
    };
    (status, body)
}

#[tokio::test]
async fn metadata_orientation_and_structure_survive_round_trips() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(&root).expect("create maintenance library");
    fs::write(root.join("root.stl"), b"root assembly").expect("write maintenance mesh");
    scanner::register_root(&pool, &root_key, "Maintenance", &root)
        .await
        .expect("register maintenance root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan maintenance root");
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let (_, files) = get_json(&router, &format!("/api/v1/roots/{root_key}/files")).await;
    let file_id = files["items"][0]["id"].as_str().expect("file ID");
    let slug = root_key.replace('_', "-");
    let (tag_status, tag) = mutation_json(
        &router,
        Method::POST,
        "/api/v1/tags",
        serde_json::json!({ "name": "CoreXY" }),
    )
    .await;
    assert_eq!(tag_status, StatusCode::CREATED);
    let tag_id = tag["id"].as_str().expect("tag ID").to_owned();
    let (parent_id, child_id, collection_id) =
        create_models_and_collection(&router, &root_key, &slug, file_id).await;
    assert_update_and_race(
        &pool,
        &router,
        &root,
        &parent_id,
        &collection_id,
        &tag_id,
        file_id,
    )
    .await;
    assert_component_history(&router, &parent_id, &child_id).await;
    assert_file_metadata(&router, &root, &parent_id, file_id).await;
    fs::remove_dir_all(root).expect("remove maintenance fixture");
}

#[tokio::test]
async fn linked_step_file_can_be_selected_as_primary_without_replacing_metadata() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (root_key, root) = unique_fixture();
    fs::create_dir_all(&root).expect("create primary fixture");
    fs::write(root.join("source.step"), b"source").expect("write source");
    fs::write(root.join("assembly.step"), b"assembly").expect("write assembly");
    scanner::register_root(&pool, &root_key, "Primary", &root)
        .await
        .expect("root");
    scanner::scan_root(&pool, &root_key, false)
        .await
        .expect("scan");
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.source_files WHERE library_root_id=(SELECT id FROM volund.library_roots WHERE root_key=$1) ORDER BY relative_path",
    ).bind(&root_key).fetch_all(&*pool).await.expect("files");
    let model_id: String = sqlx::query_scalar(
        "INSERT INTO volund.models(slug,name,description,kind) VALUES($1,'Switchwire','Retain me','project') RETURNING public_id::text",
    ).bind(root_key.replace('_', "-")).fetch_one(&*pool).await.expect("model");
    for (index, file_id) in ids.iter().enumerate() {
        sqlx::query("INSERT INTO volund.model_source_files(model_id,source_file_id,role,is_primary,ordinal) SELECT model.id,source.id,'cad',$3,$4 FROM volund.models model,volund.source_files source WHERE model.public_id::text=$1 AND source.public_id::text=$2")
            .bind(&model_id).bind(file_id).bind(index == 1)
            .bind(i32::try_from(index).expect("bounded fixture"))
            .execute(&*pool).await.expect("link");
    }
    let router = authenticated_router(&pool, api::router(pool.clone())).await;
    let (status, updated) = mutation_json(
        &router,
        Method::PUT,
        &format!("/api/v1/models/{model_id}/primary"),
        serde_json::json!({"expectedRevision":1,"primaryFileId":ids[0]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["primaryFileId"], ids[0]);
    assert_eq!(updated["description"], "Retain me");
    assert_eq!(updated["revision"], 2);
    fs::remove_dir_all(root).expect("remove primary fixture");
}

async fn assert_file_metadata(
    router: &axum::Router,
    root: &std::path::Path,
    model_id: &str,
    file_id: &str,
) {
    let uri = format!("/api/v1/models/{model_id}/files/{file_id}");
    let body = serde_json::json!({
        "expectedRevision": 1, "caption": "Primary assembly", "description": "Manufacturing source",
        "notes": "Keep original", "printable": false, "printed": true, "preSupported": false,
        "upAxis": "z", "supportHint": "No support", "orientation": [90, 0, -45]
    });
    let (status, updated) = mutation_json(router, Method::PATCH, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["caption"], "Primary assembly");
    assert_eq!(updated["revision"], 2);
    assert_eq!(
        updated["orientation"],
        serde_json::json!([90.0, 0.0, -45.0])
    );
    let (status, conflict) = mutation_json(router, Method::PATCH, &uri, body).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "revision_conflict");
    assert_eq!(
        fs::read(root.join("root.stl")).expect("read metadata-safe source"),
        b"root assembly"
    );
    if std::env::var("VOLUND_SLICER_TARGETS").is_ok()
        && std::env::var("VOLUND_PUBLIC_BASE_URL").is_ok()
    {
        assert_slicer_handoff(router, model_id, file_id).await;
    }
}

async fn assert_slicer_handoff(router: &axum::Router, model_id: &str, file_id: &str) {
    let handoff_uri = format!("/api/v1/models/{model_id}/files/{file_id}/slicer-handoff");
    let (status, rejected) = mutation_json(
        router,
        Method::POST,
        &handoff_uri,
        serde_json::json!({"targetId": "missing-target"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(rejected["error"]["message"], "unknown slicer target");

    let (status, rejected) = mutation_json(
        router,
        Method::POST,
        &handoff_uri,
        serde_json::json!({"targetId": "prusaslicer"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        rejected["error"]["message"],
        "slicer handoff requires an available printable STL or 3MF"
    );

    let printable = serde_json::json!({
        "expectedRevision": 2, "caption": "Primary assembly", "description": "Manufacturing source",
        "notes": "Keep original", "printable": true, "printed": true, "preSupported": false,
        "upAxis": "z", "supportHint": "No support", "orientation": [90, 0, -45]
    });
    let (status, _) = mutation_json(
        router,
        Method::PATCH,
        &format!("/api/v1/models/{model_id}/files/{file_id}"),
        printable,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, targets) = get_json(router, "/api/v1/slicer-targets").await;
    assert_eq!(status, StatusCode::OK);
    let target_id = targets[0]["id"].as_str().expect("slicer target ID");
    let (status, handoff) = mutation_json(
        router,
        Method::POST,
        &handoff_uri,
        serde_json::json!({"targetId": target_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        handoff["launchUrl"]
            .as_str()
            .expect("launch URL")
            .starts_with("prusaslicer://")
    );
    let base = std::env::var("VOLUND_PUBLIC_BASE_URL").expect("public base URL");
    let path = handoff["downloadUrl"]
        .as_str()
        .expect("download URL")
        .strip_prefix(&base)
        .expect("same public base");
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("handoff request"),
        )
        .await
        .expect("handoff response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"root.stl\"; filename*=UTF-8''root.stl"
    );
    assert_eq!(
        to_bytes(response.into_body(), 1024)
            .await
            .expect("handoff bytes"),
        b"root assembly".as_slice()
    );
}

async fn create_models_and_collection(
    router: &axum::Router,
    root_key: &str,
    slug: &str,
    file_id: &str,
) -> (String, String, String) {
    let (_, parent) = mutation_json(
        router, Method::POST, "/api/v1/models",
        serde_json::json!({ "name": "Root", "slug": slug, "kind": "project", "primaryFileId": file_id }),
    ).await;
    let parent_id = parent["id"].as_str().expect("parent ID").to_owned();
    let (_, child) = mutation_json(
        router, Method::POST, "/api/v1/models",
        serde_json::json!({ "name": "Toolhead", "slug": format!("{slug}-toolhead"), "kind": "assembly", "primaryFileId": file_id }),
    ).await;
    let child_id = child["id"].as_str().expect("child ID").to_owned();
    let (_, collection) = mutation_json(
        router,
        Method::POST,
        "/api/v1/collections",
        serde_json::json!({ "name": format!("Printers {root_key}"), "description": "Machines" }),
    )
    .await;
    let collection_id = collection["id"].as_str().expect("collection ID").to_owned();
    (parent_id, child_id, collection_id)
}

async fn assert_update_and_race(
    pool: &sqlx::PgPool,
    router: &axum::Router,
    root: &std::path::Path,
    parent_id: &str,
    collection_id: &str,
    tag_id: &str,
    file_id: &str,
) {
    let (status, updated) = mutation_json(
        router, Method::PATCH, &format!("/api/v1/models/{parent_id}"),
        serde_json::json!({ "expectedRevision": 1, "name": "Voron Legacy", "description": "Corrected", "kind": "project", "licenseKind": "spdx", "licenseValue": "CERN-OHL-S-2.0", "authorName": "Voron Design", "tags": [], "tagIds": [tag_id], "collectionIds": [collection_id], "primaryFileId": file_id, "viewerRotation": [-90, 0, 0] }),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Voron Legacy");
    assert_eq!(updated["authorName"], "Voron Design");
    assert_eq!(updated["licenseKind"], "spdx");
    assert_eq!(updated["licenseValue"], "CERN-OHL-S-2.0");
    assert_eq!(updated["primaryFileId"], file_id);
    assert_eq!(updated["revision"], 2);
    assert_eq!(updated["tags"][0], "CoreXY");
    assert_eq!(updated["tagIds"][0], tag_id);
    assert_eq!(
        updated["viewerRotation"],
        serde_json::json!([-90.0, 0.0, 0.0])
    );
    let (_, detail) = get_json(router, &format!("/api/v1/models/{parent_id}")).await;
    assert_eq!(detail["revision"], 2);
    let (rename_status, _) = mutation_json(
        router,
        Method::PUT,
        &format!("/api/v1/tags/{tag_id}"),
        serde_json::json!({ "expectedRevision": 1, "name": "CoreXY Stable" }),
    )
    .await;
    assert_eq!(rename_status, StatusCode::OK);
    let (_, renamed_detail) = get_json(router, &format!("/api/v1/models/{parent_id}")).await;
    assert_eq!(renamed_detail["tags"][0], "CoreXY Stable");
    assert_eq!(renamed_detail["tagIds"][0], tag_id);
    let concurrent_body = |name: &str| {
        serde_json::json!({
            "expectedRevision": 2, "name": name, "description": "Concurrent",
            "kind": "project", "licenseKind": "custom", "licenseValue": "Internal",
            "authorName": "Voron Design", "tags": [], "tagIds": [tag_id],
            "collectionIds": [collection_id], "primaryFileId": file_id,
            "viewerRotation": [-90, 0, 0]
        })
    };
    let uri = format!("/api/v1/models/{parent_id}");
    let (left, right) = tokio::join!(
        mutation_json(router, Method::PATCH, &uri, concurrent_body("Concurrent A")),
        mutation_json(router, Method::PATCH, &uri, concurrent_body("Concurrent B")),
    );
    let statuses = [left.0, right.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1
    );
    let conflict = [left, right]
        .into_iter()
        .find(|result| result.0 == StatusCode::CONFLICT)
        .expect("one stale update must conflict");
    assert_eq!(conflict.1["error"]["code"], "revision_conflict");
    assert_eq!(
        fs::read(root.join("root.stl")).expect("read unchanged source"),
        b"root assembly"
    );
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE target_type='model' \
         AND target_public_id::text=$1 AND action IN ('model.create','model.update')",
    )
    .bind(parent_id)
    .fetch_one(pool)
    .await
    .expect("count model audits");
    assert_eq!(audit_count, 3);
}

async fn assert_component_history(router: &axum::Router, parent_id: &str, child_id: &str) {
    let (status, _) = mutation_json(
        router,
        Method::POST,
        &format!("/api/v1/models/{parent_id}/components"),
        serde_json::json!({ "expectedRevision": 3, "childModelId": child_id }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, components) = get_json(router, &format!("/api/v1/models/{parent_id}/components")).await;
    assert_eq!(components[0]["name"], "Toolhead");
    let (status, conflict) = mutation_json(
        router,
        Method::POST,
        &format!("/api/v1/models/{child_id}/components"),
        serde_json::json!({ "expectedRevision": 1, "childModelId": parent_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "conflict");
    let (status, _) = mutation_json(
        router,
        Method::DELETE,
        &format!("/api/v1/models/{parent_id}/components/{child_id}"),
        serde_json::json!({ "expectedRevision": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, history) = get_json(
        router,
        &format!("/api/v1/models/{parent_id}/history?limit=100&offset=0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = history["items"].as_array().expect("model history items");
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "model.component.add")
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "model.component.remove")
    );
    assert!(
        events
            .iter()
            .all(|event| event["actorDisplayName"] == "Catalog Test Owner")
    );
    assert!(
        events
            .iter()
            .all(|event| event["change"].get("confirmation").is_none())
    );
    assert!(
        events
            .iter()
            .all(|event| event["change"].get("path").is_none())
    );
    let (status, generic) = get_json(
        router,
        &format!("/api/v1/history?targetType=model&targetId={parent_id}&limit=100&offset=0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        generic["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(generic["items"].as_array().unwrap().iter().all(|event| {
        event["actorName"] == "Catalog Test Owner"
            && event["targetType"] == "model"
            && event["summary"].get("confirmation").is_none()
            && event["summary"].get("path").is_none()
    }));
}
