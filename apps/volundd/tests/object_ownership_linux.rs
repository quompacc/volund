#![cfg(target_os = "linux")]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::session::{self, LoginInput, LoginSession, SessionPolicy};
const PASSWORD: &str = "isolated session ownership test password";
async fn call(
    router: &Router,
    method: &str,
    path: &str,
    value: Value,
    login: Option<&LoginSession>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(l) = login {
        req = req
            .header("cookie", format!("volund_session={}", l.session_token))
            .header("x-csrf-token", &l.csrf_token);
    }
    let response = router
        .clone()
        .oneshot(req.body(Body::from(value.to_string())).unwrap())
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
async fn login(db: &sqlx::PgPool, email: &str) -> LoginSession {
    session::login(
        db,
        LoginInput {
            email: email.to_owned(),
            password: PASSWORD.to_owned(),
            client_address: None,
            user_agent: None,
        },
        SessionPolicy::default(),
    )
    .await
    .unwrap()
}
async fn user(
    db: &sqlx::PgPool,
    base: &Router,
    owner: &Router,
    email: &str,
    role: &str,
) -> LoginSession {
    let (s, i) = call(
        owner,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":email,"displayName":email,"role":role}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        call(
            base,
            "POST",
            "/api/v1/invitations/accept",
            json!({"token":i["activationToken"],"password":PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    login(db, email).await
}

struct TestRoot(std::path::PathBuf);
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)] // One end-to-end draft tracks denied attempts and permitted admin cleanup.
async fn foreign_drafts_are_private_but_admin_lifecycle_controls_remain_available() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let root = TestRoot(std::env::temp_dir().join(format!(
            "volund-ownership-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(root.0.join("library")).unwrap();
    volundd::scanner::register_root(&db, "ownership", "Ownership", &root.0.join("library"))
        .await
        .unwrap();
    let library: String = sqlx::query_scalar("SELECT public_id::text FROM volund.library_roots")
        .fetch_one(&*db)
        .await
        .unwrap();
    let base =
        volundd::api::router_with_roots(db.clone(), root.0.join("derived"), root.0.join("web"));
    let owner = support::authenticated_router(&db, base.clone()).await;
    let foreign = user(&db, &base, &owner, "foreign@example.test", "editor").await;
    let draft = call(
        &owner,
        "POST",
        "/api/v1/imports/preview",
        json!({"sourceName":"Private","entries":[{"path":"private.step","byteSize":4}]}),
        None,
    )
    .await;
    assert_eq!(draft.0, StatusCode::OK);
    let id = draft.1["id"].as_str().unwrap();
    let item = draft.1["items"][0]["id"].as_str().unwrap();
    let config = json!({"modelName":"Private model","kind":"part","libraryRootId":library,"description":"","tags":[],"collectionIds":[]});
    assert_eq!(
        call(
            &owner,
            "POST",
            &format!("/api/v1/imports/{id}/metadata"),
            config.clone(),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let upload = owner
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/imports/{id}/items/{item}/content"))
                .header("content-length", "4")
                .body(Body::from("BBBB"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload.status(), StatusCode::OK);
    assert_eq!(
        call(
            &owner,
            "POST",
            &format!("/api/v1/imports/{id}/review"),
            json!({}),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    for (method, suffix, body) in [
        ("GET", String::new(), json!({})),
        ("GET", "/manifest".to_owned(), json!({})),
        ("PATCH", String::new(), json!({"displayName":"Forbidden"})),
        ("POST", "/metadata".to_owned(), config),
        ("POST", "/review".to_owned(), json!({})),
        ("POST", "/commit".to_owned(), json!({})),
        (
            "POST",
            format!("/items/{item}/resolution"),
            json!({"action":"skip"}),
        ),
        (
            "POST",
            "/cancel".to_owned(),
            json!({"confirmation":format!("CANCEL IMPORT {id}")}),
        ),
    ] {
        let result = call(
            &base,
            method,
            &format!("/api/v1/imports/{id}{suffix}"),
            body,
            Some(&foreign),
        )
        .await;
        assert_eq!(
            result.0,
            StatusCode::NOT_FOUND,
            "foreign {method} {suffix}: {:?}",
            result.1
        );
    }
    let upload = base
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/imports/{id}/items/{item}/content"))
                .header(
                    "cookie",
                    format!("volund_session={}", foreign.session_token),
                )
                .header("x-csrf-token", &foreign.csrf_token)
                .header("content-length", "4")
                .body(Body::from("XXXX"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload.status(), StatusCode::NOT_FOUND);
    let list = call(&base, "GET", "/api/v1/imports", json!({}), Some(&foreign)).await;
    assert_eq!(list.0, StatusCode::OK);
    assert!(list.1["items"].as_array().unwrap().is_empty());
    let latest = call(
        &base,
        "GET",
        "/api/v1/imports/latest-uploaded",
        json!({}),
        Some(&foreign),
    )
    .await;
    assert_eq!(latest.0, StatusCode::OK);
    assert!(latest.1.is_null());
    assert_eq!(
        std::fs::read(root.0.join("incoming").join(id).join(format!("{item}.bin"))).unwrap(),
        b"BBBB"
    );
    let unchanged: String =
        sqlx::query_scalar("SELECT status FROM volund.import_drafts WHERE public_id::text=$1")
            .bind(id)
            .fetch_one(&*db)
            .await
            .unwrap();
    assert_eq!(unchanged, "reviewed");
    // Administrative visibility and cancellation are intentional, not an ownership bypass.
    sqlx::query("UPDATE volund.users SET role='administrator' WHERE public_id::text=$1")
        .bind(&foreign.session.user_id)
        .execute(&*db)
        .await
        .unwrap();
    assert_eq!(
        call(
            &base,
            "GET",
            &format!("/api/v1/imports/{id}"),
            json!({}),
            Some(&foreign)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &base,
            "GET",
            &format!("/api/v1/imports/{id}/manifest"),
            json!({}),
            Some(&foreign)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &base,
            "POST",
            &format!("/api/v1/imports/{id}/commit"),
            json!({}),
            Some(&foreign)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &base,
            "PATCH",
            &format!("/api/v1/imports/{id}"),
            json!({"displayName":"Admin rename"}),
            Some(&foreign)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &base,
            "POST",
            &format!("/api/v1/imports/{id}/cancel"),
            json!({"confirmation":format!("CANCEL IMPORT {id}")}),
            Some(&foreign)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(!root.0.join("incoming").join(id).exists());
}
#[tokio::test]
async fn lifecycle_plan_cannot_be_consumed_by_another_administrator() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let foreign = user(
        &db,
        &base,
        &owner,
        "other-admin@example.test",
        "administrator",
    )
    .await;
    let collection = call(
        &owner,
        "POST",
        "/api/v1/collections",
        json!({"name":"Plan ownership","description":""}),
        None,
    )
    .await;
    assert_eq!(collection.0, StatusCode::CREATED);
    let plan=call(&owner,"POST","/api/v1/lifecycle/preview",json!({"action":"collection.remove","targetId":collection.1["id"],"expectedRevision":collection.1["revision"]}),None).await;
    assert_eq!(plan.0, StatusCode::OK);
    let path = format!(
        "/api/v1/lifecycle/plans/{}/apply",
        plan.1["id"].as_str().unwrap()
    );
    let body = json!({"confirmation":plan.1["confirmation"]});
    assert_eq!(
        call(&base, "POST", &path, body.clone(), Some(&foreign))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let untouched: bool =
        sqlx::query_scalar("SELECT consumed_at IS NULL FROM volund.lifecycle_plans")
            .fetch_one(&*db)
            .await
            .unwrap();
    assert!(untouched);
    assert_eq!(
        call(&owner, "POST", &path, body.clone(), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&owner, "POST", &path, body, None).await.0,
        StatusCode::NOT_FOUND
    );
}
