#![cfg(target_os = "linux")]
//! Slicer handoff safety (ADR 0031): bounded token use, re-validation at every
//! download, a per-account limit, and an explicit unavailable state.
mod support;

use std::fs;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, get_json, owner_session, test_database};
use tower::ServiceExt;
use volundd::{scanner, slicer_handoff::SlicerConfig};

const TARGETS: &str = "prusaslicer|PrusaSlicer|prusaslicer";
const BASE: &str = "https://vault.test";

/// A router whose slicer configuration is `targets` and `base`, signed in as
/// the test owner. Handoff tokens live in the database and survive the swap.
async fn configured(pool: &sqlx::PgPool, targets: &str, base: &str) -> axum::Router {
    let slicer = SlicerConfig::new(targets, base);
    owner_session(
        pool,
        volundd::slicer_handoff::router_with_config(pool.clone(), slicer),
    )
    .await
}

async fn post(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
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

async fn download(
    router: &axum::Router,
    url: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let path = url
        .strip_prefix(BASE)
        .expect("download URL uses the public base");
    let response = router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One sequential scenario owns the process environment.
async fn slicer_handoffs_are_bounded_revalidated_and_explicitly_unavailable() {
    let Some(pool) = test_database().await else {
        return;
    };
    let library = std::env::temp_dir().join(format!("volund-slicer-{}", std::process::id()));
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("plate.stl"), b"solid plate").unwrap();
    scanner::register_root(&pool, "slicer", "Slicer", &library)
        .await
        .unwrap();
    scanner::scan_root(&pool, "slicer", false).await.unwrap();
    let (model, file): (String, String) = sqlx::query_as(
        "WITH model AS (INSERT INTO volund.models (slug,name,kind) VALUES ('plate','Plate','part') \
         RETURNING id, public_id), link AS (INSERT INTO volund.model_source_files \
         (model_id,source_file_id,role,is_primary,ordinal,printable) \
         SELECT model.id,source.id,'printable-mesh',true,1,true FROM model, volund.source_files source \
         RETURNING source_file_id) \
         SELECT model.public_id::text, source.public_id::text FROM model, link \
         JOIN volund.source_files source ON source.id=link.source_file_id",
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    // Nothing configured: an empty, successful target list.
    let router = authenticated_router(
        &pool,
        volundd::slicer_handoff::router_with_config((*pool).clone(), SlicerConfig::default()),
    )
    .await;
    let (status, targets) = get_json(&router, "/api/v1/slicer-targets").await;
    assert_eq!((status, targets), (StatusCode::OK, json!([])));
    let endpoint = format!("/api/v1/models/{model}/files/{file}/slicer-handoff");
    let request = json!({"targetId": "prusaslicer"});

    // Targets without a public HTTPS base, or invalid targets: never offered.
    for (targets, base) in [(TARGETS, ""), ("evil|Shell|file", BASE)] {
        let router = configured(&pool, targets, base).await;
        let (status, body) = get_json(&router, "/api/v1/slicer-targets").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"]["code"], "slicer_handoff_misconfigured");
        let (status, body) = post(&router, &endpoint, request.clone()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    }

    let router = configured(&pool, TARGETS, BASE).await;
    let create = || post(&router, &endpoint, request.clone());
    let (status, targets) = get_json(&router, "/api/v1/slicer-targets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(targets[0]["id"], "prusaslicer");

    // A token serves a bounded number of requests and is never cached.
    let (status, handoff) = create().await;
    assert_eq!(status, StatusCode::OK, "{handoff}");
    let link = handoff["downloadUrl"].as_str().unwrap().to_owned();
    for _ in 0..8 {
        let (status, headers, bytes) = download(&router, &link).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, b"solid plate");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    }
    assert_eq!(download(&router, &link).await.0, StatusCode::NOT_FOUND);

    // Every download re-checks the reasons the handoff was allowed.
    for (revoke, restore) in [
        (
            "UPDATE volund.model_source_files SET printable=false",
            "UPDATE volund.model_source_files SET printable=true",
        ),
        (
            "UPDATE volund.models SET active=false, removed_at=now()",
            "UPDATE volund.models SET active=true, removed_at=NULL",
        ),
        (
            "UPDATE volund.source_files SET missing_at=now()",
            "UPDATE volund.source_files SET missing_at=NULL",
        ),
    ] {
        let (status, handoff) = create().await;
        assert_eq!(status, StatusCode::OK, "{handoff}");
        sqlx::query(revoke).execute(&*pool).await.unwrap();
        let link = handoff["downloadUrl"].as_str().unwrap();
        assert_eq!(
            download(&router, link).await.0,
            StatusCode::NOT_FOUND,
            "{revoke}"
        );
        sqlx::query(restore).execute(&*pool).await.unwrap();
        assert_eq!(download(&router, link).await.0, StatusCode::OK, "{restore}");
    }
    let (_, handoff) = create().await;
    let link = handoff["downloadUrl"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE volund.users SET status='disabled'")
        .execute(&*pool)
        .await
        .unwrap();
    assert_eq!(download(&router, &link).await.0, StatusCode::NOT_FOUND);
    sqlx::query("UPDATE volund.users SET status='active'")
        .execute(&*pool)
        .await
        .unwrap();
    let reconfigured = configured(&pool, "orca|Orca|orcaslicer", BASE).await;
    assert_eq!(
        download(&reconfigured, &link).await.0,
        StatusCode::NOT_FOUND,
        "a removed target invalidates its handoffs"
    );

    // One account holds a bounded number of open handoffs.
    let mut limited = None;
    for _ in 0..12 {
        let (status, body) = create().await;
        if status != StatusCode::OK {
            limited = Some((status, body));
            break;
        }
    }
    let (status, body) = limited.expect("open handoffs are bounded");
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "slicer_handoff_limit");
    let uses: i32 = sqlx::query_scalar("SELECT max(use_count) FROM volund.slicer_handoffs")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(uses, 8);

    // Retention removes handoffs that expired more than a day ago.
    sqlx::query(
        "UPDATE volund.slicer_handoffs SET created_at=now()-interval '3 days',          expires_at=now()-interval '2 days', last_used_at=NULL",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let derived = library.join("derived");
    fs::create_dir_all(&derived).unwrap();
    volundd::retention::execute(&pool, &derived, None, None)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.slicer_handoffs")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    fs::remove_dir_all(library).unwrap();
}
