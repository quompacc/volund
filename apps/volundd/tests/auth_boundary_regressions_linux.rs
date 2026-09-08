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
async fn wait_lock(db: &sqlx::PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{
 let n:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND cardinality(pg_blocking_pids(pid))>0").fetch_one(db).await.unwrap();
 if n>0 {break;}tokio::time::sleep(std::time::Duration::from_millis(20)).await;
 }}).await.unwrap();
}
#[tokio::test]
async fn protected_reads_and_head_enforce_the_declared_role_matrix() {
    use volundd::api_auth::{ROUTE_POLICIES, RoutePolicy};
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    for role in ["viewer", "editor", "administrator", "owner"] {
        // Fixture-only role selection; real role changes revoke sessions.
        sqlx::query("UPDATE volund.users SET role=$1")
            .bind(role)
            .execute(&*db)
            .await
            .unwrap();
        for rule in ROUTE_POLICIES
            .iter()
            .filter(|rule| rule.method == "GET" && rule.policy != RoutePolicy::Public)
        {
            let path = rule
                .path
                .replace("{}", "00000000-0000-0000-0000-000000000000");
            for method in ["GET", "HEAD"] {
                let response = owner
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(&path)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                if role_allows(role, rule.policy) {
                    assert_ne!(
                        response.status(),
                        StatusCode::FORBIDDEN,
                        "{role} {method} {path}"
                    );
                    assert_ne!(
                        response.status(),
                        StatusCode::UNAUTHORIZED,
                        "{role} {method} {path}"
                    );
                    if path == "/api/v1/quarantines" {
                        assert_eq!(response.status(), StatusCode::OK);
                    }
                } else {
                    assert_eq!(
                        response.status(),
                        StatusCode::FORBIDDEN,
                        "{role} {method} {path}"
                    );
                }
            }
        }
    }
    for rule in ROUTE_POLICIES
        .iter()
        .filter(|rule| rule.method == "GET" && rule.policy != RoutePolicy::Public)
    {
        let path = rule
            .path
            .replace("{}", "00000000-0000-0000-0000-000000000000");
        for method in ["GET", "HEAD"] {
            assert_eq!(
                request(&base, method, &path, json!({})).await.0,
                StatusCode::UNAUTHORIZED,
                "anonymous {method} {path}"
            );
        }
    }
    assert_eq!(
        request(
            &base,
            "GET",
            "/api/v1/slicer-download/invalid-test-token",
            json!({}),
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "the public download route must reach its token guard without a session"
    );
}

fn role_allows(role: &str, policy: volundd::api_auth::RoutePolicy) -> bool {
    use volundd::api_auth::RoutePolicy;
    match policy {
        RoutePolicy::Public | RoutePolicy::Authenticated => true,
        RoutePolicy::CatalogWrite => role != "viewer",
        RoutePolicy::UserAdmin
        | RoutePolicy::SettingsAdmin
        | RoutePolicy::LibraryAdmin
        | RoutePolicy::MetadataAdmin => matches!(role, "administrator" | "owner"),
    }
}
#[tokio::test]
async fn invitation_expiring_while_waiting_is_not_consumed() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let (s, i) = request(
        &owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"expiry@example.test","displayName":"Expiry","role":"viewer"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    sqlx::query("UPDATE volund.user_invitations SET created_at=now()-interval '1 day',expires_at=now()+interval '3 seconds'").execute(&*db).await.unwrap();
    let mut block = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM volund.users WHERE email='expiry@example.test' FOR UPDATE")
        .execute(&mut *block)
        .await
        .unwrap();
    let token = i["activationToken"].clone();
    let task = tokio::spawn(async move {
        request(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":token,"password":"valid test password for expiry"}),
        )
        .await
    });
    wait_lock(&db).await;
    sqlx::query("SELECT pg_sleep(3.2)")
        .execute(&*db)
        .await
        .unwrap();
    let expired: bool =
        sqlx::query_scalar("SELECT expires_at<clock_timestamp() FROM volund.user_invitations")
            .fetch_one(&*db)
            .await
            .unwrap();
    assert!(expired);
    block.rollback().await.unwrap();
    let result = task.await.unwrap();
    assert_eq!(result.0, StatusCode::NOT_FOUND);
    let untouched:bool=sqlx::query_scalar("SELECT u.status='invited' AND i.accepted_at IS NULL AND NOT EXISTS(SELECT 1 FROM volund.password_credentials c WHERE c.user_id=u.id) FROM volund.user_invitations i JOIN volund.users u ON u.id=i.user_id").fetch_one(&*db).await.unwrap();
    assert!(untouched);
}
#[tokio::test]
async fn metadata_changes_preserve_login_lock_until_explicit_unlock() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let (_, i) = request(
        &owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"locked@example.test","displayName":"Locked","role":"viewer"}),
    )
    .await;
    assert_eq!(
        request(
            &base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":i["activationToken"],"password":"valid test password for locked"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    for _ in 0..5 {
        assert_eq!(
            request(
                &base,
                "POST",
                "/api/v1/sessions",
                json!({"email":"locked@example.test","password":"incorrect password"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let before: bool = sqlx::query_scalar(
        "SELECT locked_until>now() FROM volund.users WHERE email='locked@example.test'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert!(before);
    let id = i["user"]["id"].as_str().unwrap();
    let r = request(
        &owner,
        "PATCH",
        &format!("/api/v1/users/{id}"),
        json!({"displayName":"Only renamed"}),
    )
    .await;
    assert_eq!(r.0, StatusCode::OK);
    assert_eq!(r.1["status"], "locked");
    let locked: bool = sqlx::query_scalar(
        "SELECT locked_until>now() FROM volund.users WHERE email='locked@example.test'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert!(locked);
    assert_eq!(
        request(
            &base,
            "POST",
            "/api/v1/sessions",
            json!({"email":"locked@example.test","password":"valid test password for locked"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let renamed_role = request(
        &owner,
        "PATCH",
        &format!("/api/v1/users/{id}"),
        json!({"role":"editor"}),
    )
    .await;
    assert_eq!(renamed_role.0, StatusCode::OK);
    assert_eq!(renamed_role.1["status"], "locked");
    let unlocked = request(
        &owner,
        "PATCH",
        &format!("/api/v1/users/{id}"),
        json!({"status":"active"}),
    )
    .await;
    assert_eq!(unlocked.0, StatusCode::OK);
    assert_eq!(unlocked.1["status"], "active");
    assert_eq!(
        request(
            &base,
            "POST",
            "/api/v1/sessions",
            json!({"email":"locked@example.test","password":"valid test password for locked"})
        )
        .await
        .0,
        StatusCode::OK
    );
}
