#![cfg(target_os = "linux")]
#![recursion_limit = "256"]
mod support;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::api_auth::{ROUTE_POLICIES, RoutePolicy};
const ID: &str = "00000000-0000-0000-0000-000000000000";

#[tokio::test]
async fn every_role_restricted_write_enforces_allowed_and_disallowed_roles() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let router = support::authenticated_router(&db, base.clone()).await;
    for rule in ROUTE_POLICIES
        .iter()
        .filter(|rule| rule.method != "GET" && rule.policy != RoutePolicy::Public)
    {
        let path = rule.path.replace("{}", ID);
        let body = payload(rule.path).to_string();
        let response = base
            .clone()
            .oneshot(
                Request::builder()
                    .method(rule.method)
                    .uri(&path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "anonymous {} {path}",
            rule.method
        );
    }
    let mut checked = 0;
    for role in ["viewer", "editor", "administrator", "owner"] {
        sqlx::query("UPDATE volund.users SET role=$1")
            .bind(role)
            .execute(&*db)
            .await
            .unwrap();
        for rule in ROUTE_POLICIES.iter().filter(|r| {
            r.method != "GET"
                && matches!(
                    r.policy,
                    RoutePolicy::CatalogWrite
                        | RoutePolicy::UserAdmin
                        | RoutePolicy::LibraryAdmin
                        | RoutePolicy::SettingsAdmin
                        | RoutePolicy::MetadataAdmin
                )
        }) {
            let path = rule.path.replace("{}", ID);
            let body = payload(rule.path);
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(rule.method)
                        .uri(&path)
                        .header("content-type", "application/json")
                        .header("content-length", body.to_string().len().to_string())
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
            if role_allows(role, rule.policy) {
                assert_ne!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{role} {} {path}",
                    rule.method
                );
                assert_ne!(
                    status,
                    StatusCode::UNAUTHORIZED,
                    "{role} {} {path}",
                    rule.method
                );
            } else {
                let expected = if rule.path == "/api/v1/lifecycle/plans/{}/apply" {
                    // Plans are first resolved by actor; existing foreign plans are
                    // covered separately by the ownership regression.
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::FORBIDDEN
                };
                assert_eq!(
                    status,
                    expected,
                    "{role} {} {}: {}",
                    rule.method,
                    path,
                    String::from_utf8_lossy(&bytes)
                );
            }
            checked += 1;
        }
    }
    println!("role-restricted write combinations checked: {checked}");
}

fn role_allows(role: &str, policy: RoutePolicy) -> bool {
    match policy {
        RoutePolicy::CatalogWrite => role != "viewer",
        RoutePolicy::UserAdmin
        | RoutePolicy::SettingsAdmin
        | RoutePolicy::LibraryAdmin
        | RoutePolicy::MetadataAdmin => matches!(role, "administrator" | "owner"),
        RoutePolicy::Public | RoutePolicy::Authenticated => true,
    }
}

fn payload(path: &str) -> Value {
    if path == "/api/v1/lifecycle/preview" {
        return json!({"expectedRevision":1,"targetId":ID,"action":"model.remove"});
    }
    if path.ends_with("/primary") {
        return json!({"expectedRevision":1,"primaryFileId":ID});
    }
    if path.ends_with("/thumbnail/regenerate") {
        return json!({"expectedRevision":1,"profile":"web"});
    }
    if path.ends_with("/problems") {
        return json!({"expectedRevision":1,"keys":["test"],"status":"resolved"});
    }
    json!({
     "name":"Role test","slug":"role-test","kind":"part","description":"","expectedRevision":1,
     "email":"role@example.test","displayName":"Role test","password":"safe isolated test password","role":"viewer","status":"active",
     "confirmation":"test confirmation","key":"role-test","path":"/tmp/volund-role-test","enabled":true,
     "nativePreset":"web","libraryId":ID,"localTime":"03:00","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,
     "value":"test","primaryFileId":ID,"childModelId":ID,"targetId":ID,"targetTagId":ID,"targetAuthorId":ID,
     "licenseKind":"unknown","viewerRotation":[0.0,0.0,0.0],"tags":[],"collectionIds":[],
     "caption":"","notes":"","printable":false,"printed":false,"preSupported":false,"supportHint":"unknown","orientation":[0.0,0.0,0.0],
     "destinationDirectory":"safe","profile":"web","sourceName":"Test","entries":[{"path":"test.step","byteSize":4}],
     "modelName":"Test","libraryRootId":ID,"action":"skip"
    })
}

#[tokio::test]
async fn lifecycle_actions_enforce_their_stricter_role_boundaries() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    for role in ["viewer", "editor", "administrator", "owner"] {
        sqlx::query("UPDATE volund.users SET role=$1")
            .bind(role)
            .execute(&*db)
            .await
            .unwrap();
        for action in [
            "model-file.unlink",
            "model.remove",
            "source.quarantine",
            "source.recover",
            "source.purge",
            "author.remove",
            "tag.remove",
            "collection.remove",
        ] {
            let allowed = match action {
                "model-file.unlink" | "model.remove" => role != "viewer",
                "source.purge" => role == "owner",
                _ => matches!(role, "administrator" | "owner"),
            };
            let response = router.clone().oneshot(Request::builder().method("POST")
                .uri("/api/v1/lifecycle/preview").header("content-type", "application/json")
                .body(Body::from(json!({"action":action,"targetId":ID,"parentId":ID,"expectedRevision":1}).to_string())).unwrap()).await.unwrap();
            // Missing fixture IDs only reach lookup for an authorized role.
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::FORBIDDEN
                },
                "{role} {action}"
            );
        }
    }
}
