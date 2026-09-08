#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::{api, retention, scanner};

mod support;

use support::{authenticated_router, get_json, test_database};

async fn mutation(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build thumbnail request"),
        )
        .await
        .expect("thumbnail request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read thumbnail response");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("thumbnail JSON")
    };
    (status, value)
}

#[tokio::test]
async fn thumbnail_candidates_are_owned_revisioned_and_retention_safe() {
    let Some(pool) = test_database().await else {
        return;
    };
    let fixture = create_thumbnail_fixture(&pool).await;
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    assert_source_thumbnail_lifecycle(&pool, &router, &fixture).await;
    assert_selected_artifact_retention(&pool, &router, &fixture).await;
    fs::remove_dir_all(fixture.root).expect("remove thumbnail fixture");
}

struct ThumbnailFixture {
    root: std::path::PathBuf,
    source_id: String,
    svg_id: String,
    model_id: String,
    foreign_model: String,
}

async fn create_thumbnail_fixture(pool: &sqlx::PgPool) -> ThumbnailFixture {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-thumbnail-{suffix}"));
    fs::create_dir_all(&root).expect("create thumbnail fixture");
    fs::write(root.join("cover.png"), b"safe raster bytes").expect("write raster");
    fs::write(root.join("drawing.svg"), b"<svg></svg>").expect("write SVG");
    scanner::register_root(pool, "thumbnail_test", "Thumbnail Test", &root)
        .await
        .expect("register root");
    scanner::scan_root(pool, "thumbnail_test", false)
        .await
        .expect("scan fixture");
    let source_id: String = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.source_files WHERE relative_path='cover.png'",
    )
    .fetch_one(pool)
    .await
    .expect("source ID");
    let svg_id: String = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.source_files WHERE relative_path='drawing.svg'",
    )
    .fetch_one(pool)
    .await
    .expect("SVG ID");
    let model_id: String = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind) VALUES ('thumbnail-model','Thumbnail model','part') RETURNING public_id::text")
        .fetch_one(pool).await.expect("model ID");
    sqlx::query("INSERT INTO volund.model_source_files (model_id,source_file_id,role,ordinal) \
        SELECT model.id,source.id,'image',(row_number() OVER (ORDER BY source.id)-1)::integer FROM volund.models model \
        CROSS JOIN volund.source_files source WHERE model.public_id::text=$1 AND source.public_id::text IN ($2,$3)")
        .bind(&model_id).bind(&source_id).bind(&svg_id).execute(pool).await.expect("associate images");
    let foreign_model: String = sqlx::query_scalar(
        "INSERT INTO volund.models (slug,name,kind) VALUES ('foreign-thumbnail','Foreign','part') RETURNING public_id::text")
        .fetch_one(pool).await.expect("foreign model");
    ThumbnailFixture {
        root,
        source_id,
        svg_id,
        model_id,
        foreign_model,
    }
}

async fn assert_source_thumbnail_lifecycle(
    pool: &sqlx::PgPool,
    router: &axum::Router,
    fixture: &ThumbnailFixture,
) {
    let (status, candidates) = get_json(
        router,
        &format!("/api/v1/models/{}/thumbnail-candidates", fixture.model_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(candidates.as_array().expect("candidate list").len(), 1);
    assert_eq!(candidates[0]["id"], fixture.source_id);
    let svg_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/files/{}/content", fixture.svg_id))
                .body(Body::empty())
                .expect("SVG request"),
        )
        .await
        .expect("SVG response");
    assert_eq!(
        svg_response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(
        svg_response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"drawing.svg\"; filename*=UTF-8''drawing.svg"
    );
    let select = serde_json::json!({"expectedRevision":1,"kind":"source-file","candidateId":fixture.source_id});
    let (status, selected) = mutation(
        router,
        &format!("/api/v1/models/{}/thumbnail", fixture.model_id),
        select,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(selected["thumbnail"]["status"], "ready");
    let (status, _) = mutation(
        router,
        &format!("/api/v1/models/{}/thumbnail", fixture.foreign_model),
        serde_json::json!({"expectedRevision":1,"kind":"source-file","candidateId":fixture.source_id}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    sqlx::query("UPDATE volund.source_files SET missing_at=now() WHERE public_id::text=$1")
        .bind(&fixture.source_id)
        .execute(pool)
        .await
        .expect("mark source missing");
    let (_, broken) = get_json(router, &format!("/api/v1/models/{}", fixture.model_id)).await;
    assert_eq!(broken["thumbnail"]["status"], "fallback");
    assert!(broken["thumbnail"]["url"].is_null());
    let uri = format!("/api/v1/models/{}/thumbnail", fixture.model_id);
    let default = serde_json::json!({"expectedRevision":2,"kind":"default","candidateId":null});
    let (left, right) = tokio::join!(
        mutation(router, &uri, default.clone()),
        mutation(router, &uri, default)
    );
    assert!([left.0, right.0].contains(&StatusCode::OK));
    assert!([left.0, right.0].contains(&StatusCode::CONFLICT));
}

async fn assert_selected_artifact_retention(
    pool: &sqlx::PgPool,
    router: &axum::Router,
    fixture: &ThumbnailFixture,
) {
    let content_id: i64 = sqlx::query_scalar(
        "SELECT content_object_id FROM volund.source_files WHERE public_id::text=$1",
    )
    .bind(&fixture.source_id)
    .fetch_one(pool)
    .await
    .expect("content ID");
    let run_id: i64 = sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,contract_version,profile,status,requested_at,started_at,finished_at) \
         VALUES ($1,'fixture','1',1,'web','ready',now()-interval '400 days',now()-interval '400 days',now()-interval '399 days') RETURNING id")
        .bind(content_id).fetch_one(pool).await.expect("conversion run");
    let artifact_id: String = sqlx::query_scalar(
        "INSERT INTO volund.derived_artifacts (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type) \
         VALUES ($1,'thumbnail-raster','fixture/thumbnail.png',$2,1,'image/png') RETURNING public_id::text")
        .bind(run_id).bind("a".repeat(64)).fetch_one(pool).await.expect("raster artifact");
    sqlx::query("UPDATE volund.source_files SET missing_at=NULL WHERE public_id::text=$1")
        .bind(&fixture.source_id)
        .execute(pool)
        .await
        .expect("restore preview source");
    let (_, generated) = get_json(router, &format!("/api/v1/models/{}", fixture.model_id)).await;
    assert_eq!(generated["thumbnail"]["kind"], "default");
    assert_eq!(generated["thumbnail"]["candidateId"], Value::Null);
    assert_eq!(generated["thumbnail"]["status"], "generated");
    assert_eq!(
        generated["thumbnail"]["url"],
        format!("/api/v1/artifacts/{artifact_id}/content")
    );
    let before = retention::preview(pool)
        .await
        .expect("retention preview before selection");
    assert_eq!(before.artifact_runs, 1);
    let uri = format!("/api/v1/models/{}/thumbnail", fixture.model_id);
    let (status, artifact_selected) = mutation(router, &uri,
        serde_json::json!({"expectedRevision":3,"kind":"derived-artifact","candidateId":artifact_id})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(artifact_selected["thumbnail"]["kind"], "derived-artifact");
    let after = retention::preview(pool)
        .await
        .expect("retention preview after selection");
    assert_eq!(after.artifact_runs, 0);
}
