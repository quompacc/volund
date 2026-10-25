#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;
use support::{assert_matches_schema, authenticated_router, test_database};

const SCHEMA: &str = include_str!("../../../contracts/metadata-export-v1.schema.json");

async fn catalog_fixture(pool: &sqlx::PgPool) -> (std::path::PathBuf, String) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let library = std::env::temp_dir().join(format!("volund-metadata-export-{suffix}"));
    fs::create_dir_all(library.join("Bauteile/clip")).expect("create library");
    fs::write(
        library.join("Bauteile/clip/clip.step"),
        b"ISO-10303-21; clip",
    )
    .expect("write STEP");
    fs::write(library.join("Bauteile/clip/cover.png"), b"raster").expect("write PNG");
    scanner::register_root(pool, "metadata_test", "Metadata Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(pool, "metadata_test", false)
        .await
        .expect("scan fixture");
    sqlx::query(
        "WITH author AS (INSERT INTO volund.authors (name,normalized_name,website) \
           VALUES ('Clip Maker','clip maker','https://example.test') RETURNING id), \
         model AS (INSERT INTO volund.models (slug,name,kind,description,author_id,license_kind,license_value) \
           SELECT 'clip','Clip','part','Small clip',id,'spdx','CC-BY-4.0' FROM author RETURNING id), \
         tag AS (INSERT INTO volund.tags (slug,name,normalized_name) \
           VALUES ('printable','Printable','printable') RETURNING id), \
         tagged AS (INSERT INTO volund.model_tags (model_id,tag_id) SELECT model.id,tag.id FROM model,tag), \
         collection AS (INSERT INTO volund.collections (slug,name,normalized_name,description) \
           VALUES ('parts','Parts','parts','All parts') RETURNING id), \
         member AS (INSERT INTO volund.collection_models (collection_id,model_id,ordinal) \
           SELECT collection.id,model.id,1 FROM collection,model) \
         INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal,caption,printable) \
         SELECT model.id,source.id, \
           CASE WHEN source.relative_path LIKE '%.step' THEN 'master-cad' ELSE 'image' END, \
           source.relative_path LIKE '%.step', row_number() OVER (ORDER BY source.relative_path), \
           'Caption '||source.relative_path, source.relative_path LIKE '%.step' \
         FROM model, volund.source_files source",
    )
    .execute(pool)
    .await
    .expect("create catalog fixture");
    sqlx::query(
        "UPDATE volund.models SET thumbnail_kind='source-file', \
         thumbnail_source_file_id=(SELECT id FROM volund.source_files WHERE relative_path LIKE '%cover.png')",
    )
    .execute(pool)
    .await
    .expect("select thumbnail");
    let model_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.models WHERE slug='clip'")
            .fetch_one(pool)
            .await
            .expect("model ID");
    sqlx::query(
        "INSERT INTO volund.security_audit_events (action,outcome,target_type,target_public_id,metadata) \
         VALUES ('model.update','success','model',$1::uuid, \
                 jsonb_build_object('fields',jsonb_build_array('name'),'revision',2,'secret','drop-me')), \
                ('session.create','success','session',NULL,'{}'::jsonb)",
    )
    .bind(&model_id)
    .execute(pool)
    .await
    .expect("write history fixture");
    (library, model_id)
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One snapshot assertion covers every exported section.
async fn metadata_export_is_versioned_complete_and_free_of_secrets() {
    let Some(pool) = test_database().await else {
        return;
    };
    let (library, model_id) = catalog_fixture(&pool).await;
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/exports/metadata")
                .body(Body::empty())
                .expect("build export request"),
        )
        .await
        .expect("metadata export");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers()["x-volund-export-format"],
        "volund-metadata-v1"
    );
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("disposition")
            .starts_with("attachment; filename=\"volund-metadata-")
    );
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("read export");
    let text = String::from_utf8(body.to_vec()).expect("UTF-8 export");
    let document: Value = serde_json::from_str(&text).expect("export JSON");
    let schema: Value = serde_json::from_str(SCHEMA).expect("schema JSON");
    assert_matches_schema(&schema, &document, "export");

    assert_eq!(document["counts"]["models"], 1);
    assert_eq!(document["counts"]["sourceFiles"], 2);
    let model = &document["models"][0];
    assert_eq!(model["id"], model_id.as_str());
    assert_eq!(model["licenseValue"], "CC-BY-4.0");
    assert_eq!(model["authorId"], document["authors"][0]["id"]);
    assert_eq!(model["tagIds"][0], document["tags"][0]["id"]);
    assert_eq!(document["collections"][0]["modelIds"][0], model_id.as_str());
    assert_eq!(model["files"].as_array().map(Vec::len), Some(2));
    let cover = document["sourceFiles"]
        .as_array()
        .expect("source files")
        .iter()
        .find(|file| file["relativePath"] == "Bauteile/clip/cover.png")
        .expect("cover source");
    assert_eq!(model["thumbnail"]["sourceFileId"], cover["id"]);
    assert_eq!(document["libraryRoots"][0]["key"], "metadata_test");
    assert!(
        document["users"]
            .as_array()
            .is_some_and(|users| !users.is_empty())
    );

    let history = document["catalogHistory"].as_array().expect("history");
    assert!(history.iter().all(|event| event["targetType"] != "session"));
    let update = history
        .iter()
        .find(|event| event["action"] == "model.update")
        .expect("model history");
    assert_eq!(update["change"]["revision"], 2);
    assert!(
        update["change"].get("secret").is_none(),
        "metadata must be allowlisted"
    );

    for forbidden in [
        library.to_str().expect("library path"),
        "password",
        "token_digest",
        "drop-me",
    ] {
        assert!(
            !text.contains(forbidden),
            "export must not contain {forbidden}"
        );
    }
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events \
         WHERE action='metadata.export' AND outcome='success' AND metadata->'counts'->>'models'='1'",
    )
    .fetch_one(&*pool)
    .await
    .expect("export audit");
    assert_eq!(audits, 1);
    fs::remove_dir_all(library).expect("remove fixture");
}
