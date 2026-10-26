#![cfg(target_os = "linux")]
//! Catalog-wide problem overview (ADR 0033).
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use support::{authenticated_router, get_json, test_database};
use tower::ServiceExt;
use volundd::scanner;

fn codes(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One fixture catalog covers every problem source.
async fn problem_overview_collects_model_file_and_library_problems() {
    let Some(pool) = test_database().await else {
        return;
    };
    let library = std::env::temp_dir().join(format!("volund-problems-{}", std::process::id()));
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(library.join("bracket.step"), b"bracket").unwrap();
    std::fs::write(library.join("gone.step"), b"gone").unwrap();
    scanner::register_root(&pool, "problems", "Problem Library", &library)
        .await
        .unwrap();
    scanner::scan_root(&pool, "problems", false).await.unwrap();
    sqlx::query(
        "INSERT INTO volund.models (slug,name,kind) VALUES ('bracket','Bracket','part'),('gone','Gone','part')",
    )
    .execute(&*pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal) \
         SELECT model.id,source.id,'master-cad',true,1 FROM volund.models model \
         JOIN volund.source_files source ON source.relative_path=model.slug||'.step'",
    )
    .execute(&*pool)
    .await
    .unwrap();
    // A finished conversion with one warning and one error diagnostic.
    sqlx::query(
        "INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,\
         contract_version,profile,status,started_at,finished_at,diagnostics) \
         SELECT source.content_object_id,'volund-cad-convert','test',1,'web','failed',now(),now(), \
         '[{\"severity\":\"warning\",\"code\":\"mesh.coarse\",\"message\":\"coarse mesh\"},\
           {\"severity\":\"error\",\"code\":\"step.invalid\",\"message\":\"broken shell\"}]'::jsonb \
         FROM volund.source_files source WHERE source.relative_path='bracket.step'",
    )
    .execute(&*pool)
    .await
    .unwrap();
    std::fs::remove_file(library.join("gone.step")).unwrap();
    scanner::scan_root(&pool, "problems", false).await.unwrap();
    sqlx::query(
        "INSERT INTO volund.scan_runs (library_root_id,status,started_at,finished_at,error_message) \
         SELECT id,'failed',now(),now(),'library volume unreadable' FROM volund.library_roots",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;

    let (status, page) = get_json(&router, "/api/v1/problems").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    // Errors first; the missing original and the failed scan are both errors.
    let found = codes(&page);
    assert_eq!(found.len(), 4, "{page}");
    assert_eq!(found[3], "mesh.coarse");
    for code in ["step.invalid", "source.missing", "scan.failed"] {
        assert!(found[..3].contains(&code), "{found:?}");
    }
    assert_eq!(page["counts"], json!({"error": 3, "warning": 1, "info": 0}));
    assert_eq!(page["truncated"], false);
    let scan = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["code"] == "scan.failed")
        .unwrap();
    assert_eq!(
        (scan["scope"].as_str(), scan["libraryKey"].as_str()),
        (Some("library"), Some("problems"))
    );
    assert!(
        scan["message"]
            .as_str()
            .unwrap()
            .contains("library volume unreadable")
    );
    let missing = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["code"] == "source.missing")
        .unwrap();
    assert_eq!(missing["modelName"], "Gone");

    let (_, errors) = get_json(&router, "/api/v1/problems?severity=error&limit=2").await;
    assert_eq!(
        (
            errors["total"].as_i64(),
            errors["items"].as_array().unwrap().len()
        ),
        (Some(3), 2)
    );

    // Ignoring a conversion problem on the model page moves it out of "open".
    let model: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.models WHERE slug='bracket'")
            .fetch_one(&*pool)
            .await
            .unwrap();
    let (_, problems) = get_json(&router, &format!("/api/v1/models/{model}/problems")).await;
    let warning = problems
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["code"] == "mesh.coarse")
        .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/v1/models/{model}/problems"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"expectedRevision": 1, "keys": [warning["key"]], "status": "ignored"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let (_, open) = get_json(&router, "/api/v1/problems?severity=info").await;
    assert!(!codes(&open).contains(&"mesh.coarse"));
    assert_eq!(open["counts"]["warning"], 0);
    let (_, ignored) = get_json(&router, "/api/v1/problems?status=ignored&severity=info").await;
    assert_eq!(codes(&ignored), ["mesh.coarse"]);
    let (_, everything) = get_json(&router, "/api/v1/problems?status=all&severity=info").await;
    assert_eq!(everything["total"], 4);

    for invalid in ["?severity=fatal", "?status=closed", "?limit=500"] {
        assert_eq!(
            get_json(&router, &format!("/api/v1/problems{invalid}"))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    std::fs::remove_dir_all(library).unwrap();
}
