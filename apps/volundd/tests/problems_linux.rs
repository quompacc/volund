#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use volundd::{api, scanner};

mod support;
use support::{authenticated_router, get_json, test_database};

#[tokio::test]
async fn model_problems_are_bounded_owned_revisioned_and_audited() {
    let Some(pool) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-problems-{suffix}"));
    fs::create_dir_all(&root).expect("create fixture");
    fs::write(root.join("broken.step"), b"broken fixture").expect("write fixture");
    scanner::register_root(&pool, "problem_test", "Problem Test", &root)
        .await
        .expect("root");
    scanner::scan_root(&pool, "problem_test", false)
        .await
        .expect("scan");
    let source_id: String = sqlx::query_scalar(
        "SELECT public_id::text FROM volund.source_files WHERE relative_path='broken.step'",
    )
    .fetch_one(&*pool)
    .await
    .expect("source");
    let model_id: String = sqlx::query_scalar("INSERT INTO volund.models (slug,name,kind) VALUES ('problem-model','Problem model','part') RETURNING public_id::text")
        .fetch_one(&*pool).await.expect("model");
    sqlx::query("INSERT INTO volund.model_source_files (model_id,source_file_id,role,ordinal,is_primary)
        SELECT model.id,source.id,'master-cad',0,true FROM volund.models model,volund.source_files source
        WHERE model.public_id::text=$1 AND source.public_id::text=$2")
        .bind(&model_id).bind(&source_id).execute(&*pool).await.expect("link");
    let content_id: i64 = sqlx::query_scalar(
        "SELECT content_object_id FROM volund.source_files WHERE public_id::text=$1",
    )
    .bind(&source_id)
    .fetch_one(&*pool)
    .await
    .expect("content");
    let preview_id: String = sqlx::query_scalar("INSERT INTO volund.conversion_runs
        (content_object_id,converter_name,converter_version,contract_version,profile,status,requested_at,started_at,finished_at,diagnostics)
        VALUES ($1,'fixture','1',1,'fine','failed',now(),now(),now(),$2) RETURNING public_id::text")
        .bind(content_id).bind(serde_json::json!([{"severity":"error","code":"mesh.invalid","message":"Malformed <mesh>"}]))
        .fetch_one(&*pool).await.expect("run");
    let derived = root.join("derived");
    install_invalid_manifest(&pool, &derived, &preview_id).await;
    let router = authenticated_router(
        &pool,
        api::router_with_roots((*pool).clone(), derived.clone(), root.clone()),
    )
    .await;
    let uri = format!("/api/v1/models/{model_id}/problems");
    let (status, listed) = get_json(&router, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["key"], format!("{preview_id}:0"));
    assert_eq!(listed[0]["status"], "open");
    assert_eq!(listed[0]["message"], "Malformed <mesh>");
    assert!(
        listed
            .as_array()
            .expect("problem array")
            .iter()
            .any(|problem| {
                problem["code"] == "assembly.manifest.invalid"
                    && problem["message"]
                        == "Das STEP-Strukturmanifest enthält doppelte Knoten-IDs."
            })
    );
    let (status, updated) = put(
        &router,
        &uri,
        serde_json::json!({
            "expectedRevision":1,"keys":[format!("{preview_id}:0")],"status":"ignored"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated[0]["status"], "ignored");
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='model.problem.status' AND target_public_id::text=$1")
        .bind(&model_id).fetch_one(&*pool).await.expect("audit");
    assert_eq!(audit, 1);
    let (foreign, _) = put(&router, &uri, serde_json::json!({
        "expectedRevision":1,"keys":["00000000-0000-0000-0000-000000000000:0"],"status":"resolved"
    })).await;
    assert_eq!(foreign, StatusCode::BAD_REQUEST);
    install_valid_replacement_manifest(&pool, &derived, content_id).await;
    let (status, after_regeneration) = get_json(&router, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after_regeneration, serde_json::json!([]));
    assert_manifest_conventions(&pool, &router, &uri, &derived).await;
    assert_eq!(
        fs::read(root.join("broken.step")).expect("original"),
        b"broken fixture"
    );
    let historical: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.conversion_runs WHERE public_id::text=$1")
            .bind(&preview_id)
            .fetch_one(&*pool)
            .await
            .expect("historical run retained");
    assert_eq!(historical, 1);
    fs::remove_dir_all(root).expect("remove fixture");
}

async fn assert_manifest_conventions(
    pool: &sqlx::PgPool,
    router: &axum::Router,
    uri: &str,
    derived: &std::path::Path,
) {
    let path = derived.join("jobs/replacement/assembly.json");
    let valid = fs::read(&path).expect("valid manifest bytes");
    let original: Value = serde_json::from_slice(&valid).expect("valid JSON");
    for (field, invalid) in [
        ("contractVersion", serde_json::json!(0)),
        ("transformConvention", Value::Null),
        ("transformConvention", serde_json::json!("column-major")),
        ("colorSpace", Value::Null),
        ("colorSpace", serde_json::json!("linear")),
    ] {
        let mut legacy = original.clone();
        if invalid.is_null() {
            legacy.as_object_mut().expect("object").remove(field);
        } else {
            legacy[field] = invalid;
        }
        let bytes = serde_json::to_vec(&legacy).expect("legacy JSON");
        fs::write(&path, &bytes).expect("legacy artifact");
        sqlx::query("UPDATE volund.derived_artifacts SET byte_size=$1,sha256=$2 WHERE relative_path='jobs/replacement/assembly.json'")
            .bind(i64::try_from(bytes.len()).expect("bounded fixture"))
            .bind(format!("{:x}", Sha256::digest(&bytes)))
            .execute(pool).await.expect("consistent legacy metadata");
        let (status, problems) = get_json(router, uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            problems[0]["code"], "assembly.manifest.invalid",
            "{field}: {problems}"
        );
        assert_eq!(problems[0]["severity"], "error");
        let (status, served) = get_json(
            router,
            problems[0]["diagnosticsUrl"]
                .as_str()
                .expect("artifact URL"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(served, legacy);
        assert_eq!(
            problems[0]["remediation"],
            "STEP-Vorschau mit dem aktuellen Konverter neu erzeugen."
        );
        fs::write(&path, &valid).expect("restore valid artifact");
        sqlx::query("UPDATE volund.derived_artifacts SET byte_size=$1,sha256=$2 WHERE relative_path='jobs/replacement/assembly.json'")
            .bind(i64::try_from(valid.len()).expect("bounded fixture"))
            .bind(format!("{:x}", Sha256::digest(&valid)))
            .execute(pool).await.expect("consistent restored metadata");
        let (status, restored) = get_json(router, uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(restored, serde_json::json!([]));
    }
}

async fn install_invalid_manifest(
    pool: &sqlx::PgPool,
    derived: &std::path::Path,
    preview_id: &str,
) {
    let artifact_directory = derived.join("jobs/fixture");
    fs::create_dir_all(&artifact_directory).expect("create derived fixture");
    let assembly = serde_json::json!({
        "contractVersion":1,
        "transformConvention":"row-major, parent-local", "colorSpace":"sRGB",
        "definitions":[{"id":"part"}],
        "roots":[
            {"id":"duplicate","definition":"part","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]},
            {"id":"duplicate","definition":"part","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]}
        ]
    });
    let bytes = serde_json::to_vec(&assembly).expect("assembly JSON");
    fs::write(artifact_directory.join("assembly.json"), &bytes).expect("assembly artifact");
    let byte_size = i64::try_from(bytes.len()).expect("bounded fixture");
    sqlx::query(
        "INSERT INTO volund.derived_artifacts
        (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type)
        SELECT id,'assembly-manifest','jobs/fixture/assembly.json',$2,$3,'application/json'
        FROM volund.conversion_runs WHERE public_id::text=$1",
    )
    .bind(preview_id)
    .bind("0".repeat(64))
    .bind(byte_size)
    .execute(pool)
    .await
    .expect("artifact row");
}

async fn install_valid_replacement_manifest(
    pool: &sqlx::PgPool,
    derived: &std::path::Path,
    content_id: i64,
) {
    let preview_id: String = sqlx::query_scalar(
        "INSERT INTO volund.conversion_runs
        (content_object_id,converter_name,converter_version,contract_version,profile,status,requested_at,started_at,finished_at,diagnostics)
        VALUES ($1,'fixture','2',1,'fine','ready',now(),now(),now(),'[]') RETURNING public_id::text",
    )
    .bind(content_id)
    .fetch_one(pool)
    .await
    .expect("replacement run");
    let artifact_directory = derived.join("jobs/replacement");
    fs::create_dir_all(&artifact_directory).expect("create replacement fixture");
    let assembly = serde_json::json!({
        "contractVersion":1,
        "transformConvention":"row-major, parent-local", "colorSpace":"sRGB",
        "definitions":[
            {"id":"assembly","name":"Assembly","kind":"assembly","color":null},
            {"id":"part","name":"Part","kind":"part","color":null}
        ],
        "roots":[{
            "id":"node-1","name":"Assembly","kind":"assembly","color":null,
            "definition":"assembly","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],
            "children":[{"id":"node-2","name":"Part","kind":"part","color":null,"definition":"part","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]}]
        }]
    });
    let bytes = serde_json::to_vec(&assembly).expect("replacement assembly JSON");
    fs::write(artifact_directory.join("assembly.json"), &bytes)
        .expect("replacement assembly artifact");
    sqlx::query(
        "INSERT INTO volund.derived_artifacts
        (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type)
        SELECT id,'assembly-manifest','jobs/replacement/assembly.json',$2,$3,'application/json'
        FROM volund.conversion_runs WHERE public_id::text=$1",
    )
    .bind(preview_id)
    .bind(format!("{:x}", Sha256::digest(&bytes)))
    .bind(i64::try_from(bytes.len()).expect("bounded replacement fixture"))
    .execute(pool)
    .await
    .expect("replacement artifact row");
}

async fn put(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    (status, serde_json::from_slice(&bytes).expect("json"))
}
