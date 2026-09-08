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
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tower::ServiceExt;
use volundd::{scanner, storage_health};

struct Fixture {
    db: support::TestDatabase,
    router: Router,
    root: PathBuf,
    id: String,
}

impl Fixture {
    async fn new(parent: &Path) -> Option<Self> {
        let db = support::test_database().await?;
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = parent.join(format!("volund-storage-failure-{suffix}"));
        fs::create_dir_all(root.join("library/target")).unwrap();
        fs::create_dir_all(root.join("incoming")).unwrap();
        fs::write(
            root.join("library/original.step"),
            b"protected original bytes",
        )
        .unwrap();
        scanner::register_root(&db, "probe", "Probe", &root.join("library"))
            .await
            .unwrap();
        scanner::scan_root(&db, "probe", false).await.unwrap();
        let id = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
            .fetch_one(&*db)
            .await
            .unwrap();
        let router = support::authenticated_router(
            &db,
            volundd::api::router_with_roots(db.clone(), root.join("derived"), root.join("web")),
        )
        .await;
        Some(Self {
            db,
            router,
            root,
            id,
        })
    }

    async fn move_original(&self) -> (StatusCode, Value) {
        post(
            &self.router,
            &format!("/api/v1/files/{}/move", self.id),
            json!({"destinationDirectory":"target"}),
        )
        .await
    }

    async fn assert_original(&self, path: &str, moves: i64) {
        let rows: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT public_id::text,relative_path,missing_at IS NULL FROM volund.source_files",
        )
        .fetch_all(&*self.db)
        .await
        .unwrap();
        assert_eq!(rows, vec![(self.id.clone(), path.into(), true)]);
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM volund.source_file_moves),
                    (SELECT count(*) FROM volund.source_cleanup)",
        )
        .fetch_one(&*self.db)
        .await
        .unwrap();
        assert_eq!(counts, (moves, 0));
        assert_eq!(
            fs::read(self.root.join("library").join(path)).unwrap(),
            b"protected original bytes"
        );
    }

    async fn draft(&self) -> (String, String) {
        let (status, draft) = post(
            &self.router,
            "/api/v1/imports/preview",
            json!({
                "sourceName":"Storage failure", "entries":[{"path":"new.step","byteSize":65536}]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{draft}");
        let id = draft["id"].as_str().unwrap().to_owned();
        let item = draft["items"][0]["id"].as_str().unwrap().to_owned();
        let library: String =
            sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots")
                .fetch_one(&*self.db)
                .await
                .unwrap();
        let (status, body) = post(
            &self.router,
            &format!("/api/v1/imports/{id}/metadata"),
            json!({
                "modelName":"Storage failure", "kind":"part", "libraryRootId":library,
                "description":"", "tags":[], "collectionIds":[]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (id, item)
    }

    async fn upload(&self, draft: &str, item: &str) -> (StatusCode, Value) {
        request(
            &self.router,
            &format!("/api/v1/imports/{draft}/items/{item}/content"),
            "application/octet-stream",
            vec![7; 65536],
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for path in ["library", "library/target"] {
            let _ = fs::set_permissions(self.root.join(path), fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn write_protected_library_is_readable_but_move_fails_and_can_retry() {
    let Some(f) = Fixture::new(&std::env::temp_dir()).await else {
        return;
    };
    let library = f.root.join("library");
    for path in [&library, &library.join("target")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }
    let observation = storage_health::probe(&library);
    assert!(observation.readable);
    assert!(
        !observation.writable,
        "test must run without root/DAC bypass"
    );
    assert!(
        observation
            .reasons
            .iter()
            .any(|r| r.code == "storage_not_writable")
    );
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    let (status, body) = f.move_original().await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot stage lifecycle link")
    );
    f.assert_original("original.step", 0).await;
    assert!(!library.join("target/original.step").exists());
    for path in [&library, &library.join("target")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(f.move_original().await.0, StatusCode::OK);
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_original("target/original.step", 1).await;
}

#[tokio::test]
async fn unavailable_library_never_marks_original_missing_or_reports_move_success() {
    let Some(f) = Fixture::new(&std::env::temp_dir()).await else {
        return;
    };
    let library = f.root.join("library");
    let offline = f.root.join("offline");
    fs::rename(&library, &offline).unwrap();
    let observation = storage_health::probe(&library);
    assert_eq!(observation.state, storage_health::OperationalState::Blocked);
    assert!(
        observation
            .reasons
            .iter()
            .any(|r| r.code == "storage_unreachable")
    );
    let scan_error = scanner::scan_root(&f.db, "probe", false).await.unwrap_err();
    assert!(!scan_error.is_empty());
    let (status, body) = f.move_original().await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"]["message"], "managed move failed");
    assert!(!library.exists(), "no replacement library may be created");
    assert_eq!(
        fs::read(offline.join("original.step")).unwrap(),
        b"protected original bytes"
    );
    fs::rename(&offline, &library).unwrap();
    f.assert_original("original.step", 0).await;
    assert_eq!(f.move_original().await.0, StatusCode::OK);
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_original("target/original.step", 1).await;
}

#[tokio::test]
#[ignore = "requires the bounded private tmpfs harness tests/storage-volume-linux.sh"]
async fn full_volume_upload_fails_without_original_loss_and_can_retry() {
    let volume = PathBuf::from(
        std::env::var("VOLUND_TEST_FULL_VOLUME").expect("private test volume required"),
    );
    let volume = fs::canonicalize(volume).unwrap();
    assert!(
        volume
            .to_str()
            .unwrap()
            .starts_with("/var/tmp/volund-storage-test.")
    );
    let observation = storage_health::probe(&volume);
    assert_eq!(observation.filesystem_type.as_deref(), Some("tmpfs"));
    assert!(
        observation
            .total_bytes
            .is_some_and(|size| size <= 4 * 1024 * 1024)
    );
    let f = Fixture::new(&volume)
        .await
        .expect("isolated test database required");
    let (draft, item) = f.draft().await;
    fs::create_dir_all(f.root.join("incoming").join(&draft)).unwrap();
    let filler = f.root.join("filler");
    fill_bounded_volume(&filler);
    let full = storage_health::probe(&volume);
    assert_eq!(full.available_bytes, Some(0));
    assert_eq!(full.state, storage_health::OperationalState::Blocked);
    assert!(
        full.reasons
            .iter()
            .any(|r| r.code == "storage_capacity_blocked")
    );
    let (status, body) = f.upload(&draft, &item).await;
    if status.is_success() {
        let published = f
            .root
            .join("incoming")
            .join(&draft)
            .join(format!("{item}.bin"));
        eprintln!(
            "unexpected upload success on full volume: published bytes={:?}",
            fs::metadata(published).map(|metadata| metadata.len())
        );
    }
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"]["message"], "import upload failed");
    let state: (String, i64, bool, bool) = sqlx::query_as(
        "SELECT upload_status,uploaded_bytes,sha256 IS NULL,upload_started_at IS NULL
         FROM volund.import_draft_items",
    )
    .fetch_one(&*f.db)
    .await
    .unwrap();
    assert_eq!(state, ("pending".into(), 0, true, true));
    assert_eq!(
        fs::read_dir(f.root.join("incoming").join(&draft))
            .unwrap()
            .count(),
        0
    );
    f.assert_original("original.step", 0).await;
    fs::remove_file(filler).unwrap();
    let (status, uploaded) = f.upload(&draft, &item).await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    assert_eq!(uploaded["status"], "uploaded");
    assert_eq!(
        fs::read(
            f.root
                .join("incoming")
                .join(&draft)
                .join(format!("{item}.bin"))
        )
        .unwrap(),
        vec![7; 65536]
    );
    f.assert_original("original.step", 0).await;
}

fn fill_bounded_volume(path: &Path) {
    let mut file = fs::File::create(path).unwrap();
    let buffer = vec![0; 65536];
    for _ in 0..128 {
        if let Err(error) = file.write_all(&buffer) {
            assert_eq!(
                error.raw_os_error(),
                Some(28),
                "expected real ENOSPC: {error}"
            );
            return;
        }
    }
    panic!("test volume did not fill within the hard 8 MiB write limit");
}

async fn post(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request(
        router,
        uri,
        "application/json",
        body.to_string().into_bytes(),
    )
    .await
}

async fn request(
    router: &Router,
    uri: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", content_type)
                .header("content-length", bytes.len())
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
