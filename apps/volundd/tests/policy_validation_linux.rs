#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(r: &Router, method: &str, path: &str, v: Value) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let b = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        if b.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&b).unwrap()
        },
    )
}

#[tokio::test]
async fn profiles_reject_invalid_values_and_protect_system_defaults() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let base = json!({"name":"Boundary profile","nativePreset":"web","linearDeflection":null,"angularDeflection":null,"enabled":true,"confirmation":"APPLY PROFILE new"});
    let (_, before) = request(&router, "GET", "/api/v1/conversion-profiles", json!({})).await;
    for (field, value) in [
        ("name", json!("")),
        ("name", json!(" leading")),
        ("name", json!("x".repeat(81))),
        ("nativePreset", json!("unknown")),
        ("linearDeflection", json!(0)),
        ("linearDeflection", json!(1001)),
        ("angularDeflection", json!(0)),
        ("angularDeflection", json!(3.2)),
    ] {
        let mut input = base.clone();
        input[field] = value;
        assert_eq!(
            request(&router, "POST", "/api/v1/conversion-profiles", input)
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{field}"
        );
    }
    let (_, unchanged) = request(&router, "GET", "/api/v1/conversion-profiles", json!({})).await;
    assert_eq!(before, unchanged);
    for profile in before
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["builtIn"] == true)
    {
        let id = profile["id"].as_str().unwrap();
        let path = format!("/api/v1/conversion-profiles/{id}");
        let mut input = base.clone();
        input["expectedRevision"] = profile["revision"].clone();
        input["confirmation"] = json!(format!("APPLY PROFILE {id}"));
        assert_eq!(
            request(&router, "PUT", &path, input).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &router,
                "DELETE",
                &path,
                json!({"confirmation":format!("RETIRE {id}"),"expectedRevision":profile["revision"]})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert!(
        before
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["builtIn"] == true)
    );
    let (status, created) =
        request(&router, "POST", "/api/v1/conversion-profiles", base.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/conversion-profiles/{id}");
    let mut update = base;
    update["enabled"] = json!(false);
    update["expectedRevision"] = created["revision"].clone();
    update["confirmation"] = json!(format!("APPLY PROFILE {id}"));
    let (status, changed) = request(&router, "PUT", &path, update.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        request(&router, "PUT", &path, update).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, listed) = request(&router, "GET", "/api/v1/conversion-profiles", json!({})).await;
    assert_eq!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap(),
        &changed
    );
    for system in before.as_array().unwrap() {
        assert!(listed.as_array().unwrap().contains(system));
    }
}

#[tokio::test]
async fn schedules_reject_invalid_values_and_delete_only_after_confirmation() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let root: String = sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('validation','Validation','/var/tmp/unused-policy-validation') RETURNING public_id::text").fetch_one(&*db).await.unwrap();
    let base = json!({"libraryId":root,"name":"Night","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true,"confirmation":"APPLY SCHEDULE new"});
    for (field, value) in [
        ("name", json!("")),
        ("name", json!("trailing ")),
        ("name", json!("x".repeat(81))),
        ("localTime", json!("25:00")),
        ("localTime", json!("02:60")),
        ("timeZone", json!("Unknown/Zone")),
        ("weekdayMask", json!(0)),
        ("weekdayMask", json!(128)),
        ("libraryId", json!("00000000-0000-0000-0000-000000000000")),
    ] {
        let mut input = base.clone();
        input[field] = value;
        let (status, body) = request(&router, "POST", "/api/v1/scan-schedules", input).await;
        assert!(status.is_client_error(), "{field}: {status} {body}");
    }
    let (_, empty) = request(&router, "GET", "/api/v1/scan-schedules", json!({})).await;
    assert_eq!(empty, json!([]));
    let (status, created) = request(&router, "POST", "/api/v1/scan-schedules", base).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/scan-schedules/{id}");
    assert_eq!(
        request(
            &router,
            "DELETE",
            &path,
            json!({"confirmation":"wrong","expectedRevision":1})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, listed) = request(&router, "GET", "/api/v1/scan-schedules", json!({})).await;
    assert_eq!(listed, json!([created]));
    assert_eq!(
        request(
            &router,
            "DELETE",
            &path,
            json!({"confirmation":format!("DELETE SCHEDULE {id}"),"expectedRevision":1})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, empty) = request(&router, "GET", "/api/v1/scan-schedules", json!({})).await;
    assert_eq!(empty, json!([]));
}
