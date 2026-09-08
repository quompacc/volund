#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};
use tower::ServiceExt;

async fn request(router: &Router, route: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(route)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn directory() -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("volund-path-boundary-{suffix}"));
    fs::create_dir(&path).unwrap();
    path
}

#[tokio::test]
async fn invalid_paths_are_rejected_without_registration() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let path = directory();
    let file = path.join("original.step");
    fs::write(&file, b"unchanged test original").unwrap();
    for invalid in [
        String::new(),
        "relative/path".into(),
        "C:\\cad".into(),
        format!(" {}", path.display()),
        path.join("missing").to_str().unwrap().into(),
        file.to_str().unwrap().into(),
        "/nul\0path".into(),
        format!("/{}", "a".repeat(4096)),
    ] {
        for route in ["/api/v1/libraries/validate", "/api/v1/libraries"] {
            let (status, body) = request(&router, route, json!({"path":invalid,"key":"invalid","name":"Invalid","confirmation":"ADD LIBRARY invalid"})).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body}");
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.library_roots")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(fs::read(&file).unwrap(), b"unchanged test original");
    fs::remove_file(file).unwrap();
    fs::remove_dir(path).unwrap();
}

#[tokio::test]
async fn unreadable_or_unsearchable_directories_are_rejected() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    for mode in [0o000, 0o400] {
        let path = directory();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        let validated = request(&router, "/api/v1/libraries/validate", json!({"path":path})).await;
        let created = request(&router, "/api/v1/libraries", json!({"path":path,"key":"blocked","name":"Blocked","confirmation":"ADD LIBRARY blocked"})).await;
        let storage = volundd::storage_health::probe(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir(&path).unwrap();
        assert_eq!(
            validated.0,
            StatusCode::BAD_REQUEST,
            "mode={mode:o}: {}",
            validated.1
        );
        assert_eq!(
            created.0,
            StatusCode::BAD_REQUEST,
            "mode={mode:o}: {}",
            created.1
        );
        assert!(!storage.readable);
        assert!(
            storage
                .reasons
                .iter()
                .any(|r| r.code == "storage_unreadable")
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.library_roots")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn readable_searchable_read_only_directory_remains_valid() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let path = directory();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
    let validated = request(&router, "/api/v1/libraries/validate", json!({"path":path})).await;
    let created = request(&router, "/api/v1/libraries", json!({"path":path,"key":"readonly","name":"Read only","confirmation":"ADD LIBRARY readonly"})).await;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(path).unwrap();
    assert_eq!(validated.0, StatusCode::OK);
    assert_eq!(validated.1["readable"], true);
    assert_eq!(validated.1["writablePermission"], false);
    assert_eq!(created.0, StatusCode::OK);
    assert_eq!(created.1["readOnly"], true);
    assert_eq!(created.1["storage"]["readable"], true);
    assert!(
        !created.1["storage"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "storage_unreadable")
    );
}

#[tokio::test]
async fn duplicate_keys_and_canonical_aliases_preserve_existing_library() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let path = directory();
    let other = path.join("other");
    fs::create_dir(&other).unwrap();
    let alias = path.join("alias");
    symlink(&other, &alias).unwrap();
    let (status, created) = request(
        &router,
        "/api/v1/libraries",
        json!({"path":other,"key":"stable","name":"Stable","confirmation":"ADD LIBRARY stable"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for (key, duplicate) in [
        ("stable", path.clone()),
        ("duplicate", other.clone()),
        ("alias", alias.clone()),
        ("dot", other.join(".")),
    ] {
        let (status, body) = request(&router, "/api/v1/libraries", json!({"path":duplicate,"key":key,"name":"Replacement","confirmation":format!("ADD LIBRARY {key}")})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
    }
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT public_id::text, display_name, filesystem_path FROM volund.library_roots",
    )
    .fetch_all(&*db)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(
            created["id"].as_str().unwrap().into(),
            "Stable".into(),
            other.to_str().unwrap().into()
        )]
    );
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='library.create' AND outcome='success'").fetch_one(&*db).await.unwrap();
    assert_eq!(audits, 1);
    fs::remove_file(alias).unwrap();
    fs::remove_dir(other).unwrap();
    fs::remove_dir(path).unwrap();
}
