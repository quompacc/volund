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
#[tokio::test]
async fn concurrent_owner_changes_are_serialized() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let b = user(&db, &base, &owner, "owner-b@example.test", "owner").await;
    let c = user(&db, &base, &owner, "owner-c@example.test", "owner").await;
    // Hold the first owner so both requests reach an observable lock barrier.
    let mut barrier = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM volund.users WHERE email='catalog-owner@example.test' FOR UPDATE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let r1 = owner.clone();
    let r2 = owner.clone();
    let one = tokio::spawn(async move {
        call(
            &r1,
            "PATCH",
            &format!("/api/v1/users/{}", b.session.user_id),
            json!({"role":"viewer"}),
            None,
        )
        .await
    });
    let two = tokio::spawn(async move {
        call(
            &r2,
            "PATCH",
            &format!("/api/v1/users/{}", c.session.user_id),
            json!({"role":"viewer"}),
            None,
        )
        .await
    });
    wait_for_waiters(&db, 2).await;
    barrier.rollback().await.unwrap();
    let a = one.await.unwrap();
    let b = two.await.unwrap();
    println!("parallel owner demotion statuses: {} / {}", a.0, b.0);
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::OK);
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.users WHERE role='owner' AND status='active'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(owners, 1);
    println!("active owners preserved={owners}");
}

#[tokio::test]
async fn concurrent_mutual_demotion_preserves_last_owner() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let first_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.users WHERE role='owner'")
            .fetch_one(&*db)
            .await
            .unwrap();
    let second = user(&db, &base, &owner, "second-owner@example.test", "owner").await;
    let second_id = second.session.user_id.clone();
    let mut barrier = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM volund.users WHERE email='catalog-owner@example.test' FOR UPDATE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let one = tokio::spawn(async move {
        call(
            &owner,
            "PATCH",
            &format!("/api/v1/users/{second_id}"),
            json!({"role":"viewer"}),
            None,
        )
        .await
    });
    wait_for_waiters(&db, 1).await;
    let two = tokio::spawn(async move {
        call(
            &base,
            "PATCH",
            &format!("/api/v1/users/{first_id}"),
            json!({"role":"viewer"}),
            Some(&second),
        )
        .await
    });
    wait_for_waiters(&db, 2).await;
    barrier.rollback().await.unwrap();
    let first = one.await.unwrap();
    let second = two.await.unwrap();
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::CONFLICT);
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.users WHERE role='owner' AND status='active'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(remaining, 1);
}
async fn wait_for_waiters(db: &sqlx::PgPool, minimum: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            // Other tests wait on the fixture lock. Only account-change SQL
            // proves that our requests reached the intended transaction barrier.
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() \
                 AND wait_event_type='Lock' AND cardinality(pg_blocking_pids(pid))>0 \
                 AND (query LIKE 'SELECT pg_advisory_xact_lock(860756368,4)%' \
                 OR query LIKE 'SELECT id FROM volund.users WHERE role = %FOR UPDATE' \
                 OR query LIKE 'SELECT id, public_id::text, display_name, role, status FROM volund.users %FOR UPDATE')",
            ).fetch_one(db).await.unwrap();
            if n >= minimum {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn idle_and_absolute_expiry_refuse_access_without_reviving_sessions() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let first = user(&db, &base, &owner, "expiry@example.test", "editor").await;
    sqlx::query("UPDATE volund.sessions SET idle_expires_at=now()+interval '10 minutes', absolute_expires_at=now()+interval '30 minutes' WHERE public_id::text=$1")
        .bind(&first.session.id).execute(&*db).await.unwrap();
    assert_eq!(
        call(&base, "GET", "/api/v1/session", json!({}), Some(&first))
            .await
            .0,
        StatusCode::OK
    );
    let capped: bool = sqlx::query_scalar(
        "SELECT idle_expires_at=absolute_expires_at FROM volund.sessions WHERE public_id::text=$1",
    )
    .bind(&first.session.id)
    .fetch_one(&*db)
    .await
    .unwrap();
    assert!(
        capped,
        "activity must not extend the absolute session lifetime"
    );
    for (absolute, current) in [
        (false, first),
        (true, login(&db, "expiry@example.test").await),
    ] {
        assert_eq!(
            call(&base, "GET", "/api/v1/models", json!({}), Some(&current))
                .await
                .0,
            StatusCode::OK
        );
        // Move only this disposable session's clock boundaries into the past.
        sqlx::query("UPDATE volund.sessions SET created_at=now()-interval '2 days', last_seen_at=now()-interval '1 day', idle_expires_at=now()-interval '1 hour', absolute_expires_at=CASE WHEN $2 THEN now()-interval '30 minutes' ELSE now()+interval '1 day' END WHERE public_id::text=$1")
            .bind(&current.session.id).bind(absolute).execute(&*db).await.unwrap();
        for path in ["/api/v1/session", "/api/v1/models"] {
            assert_eq!(
                call(&base, "GET", path, json!({}), Some(&current)).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        let untouched: bool = sqlx::query_scalar("SELECT last_seen_at<now()-interval '23 hours' AND idle_expires_at<now() FROM volund.sessions WHERE public_id::text=$1")
            .bind(&current.session.id).fetch_one(&*db).await.unwrap();
        assert!(untouched);
    }
    let fresh = login(&db, "expiry@example.test").await;
    assert_eq!(
        call(&base, "GET", "/api/v1/models", json!({}), Some(&fresh))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn role_and_status_changes_revoke_all_old_sessions_and_enforce_new_access() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let first = user(&db, &base, &owner, "transition@example.test", "editor").await;
    let second = login(&db, "transition@example.test").await;
    assert_private_sessions(&base, &owner, &first, &second).await;
    let path = format!("/api/v1/users/{}", first.session.user_id);
    assert_eq!(
        call(&owner, "PATCH", &path, json!({"role":"viewer"}), None)
            .await
            .0,
        StatusCode::OK
    );
    for previous in [&first, &second] {
        assert_eq!(
            call(&base, "GET", "/api/v1/models", json!({}), Some(previous))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let viewer = login(&db, "transition@example.test").await;
    assert_eq!(viewer.session.role, "viewer");
    assert_eq!(
        call(&base, "GET", "/api/v1/models", json!({}), Some(&viewer))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &base,
            "POST",
            "/api/v1/collections",
            json!({"name":"Forbidden collection","description":""}),
            Some(&viewer)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&owner, "PATCH", &path, json!({"status":"disabled"}), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&base, "GET", "/api/v1/session", json!({}), Some(&viewer))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &base,
            "POST",
            "/api/v1/sessions",
            json!({"email":"transition@example.test","password":PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&owner, "PATCH", &path, json!({"status":"active"}), None)
            .await
            .0,
        StatusCode::OK
    );
    for previous in [&first, &second, &viewer] {
        assert_eq!(
            call(&base, "GET", "/api/v1/session", json!({}), Some(previous))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let fresh = login(&db, "transition@example.test").await;
    assert_eq!(
        call(&base, "GET", "/api/v1/models", json!({}), Some(&fresh))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn owner_and_administrator_cannot_lock_themselves_out() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let owner = support::authenticated_router(&db, base.clone()).await;
    let owner_id: String =
        sqlx::query_scalar("SELECT public_id::text FROM volund.users WHERE role='owner'")
            .fetch_one(&*db)
            .await
            .unwrap();
    let admin = user(
        &db,
        &base,
        &owner,
        "self-admin@example.test",
        "administrator",
    )
    .await;
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action='user.update'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    for (router, id, credentials) in [
        (&owner, &owner_id, None),
        (&base, &admin.session.user_id, Some(&admin)),
    ] {
        for payload in [
            json!({"role":"viewer"}),
            json!({"status":"disabled"}),
            json!({"status":"locked"}),
        ] {
            let result = call(
                router,
                "PATCH",
                &format!("/api/v1/users/{id}"),
                payload,
                credentials,
            )
            .await;
            assert_eq!(result.0, StatusCode::FORBIDDEN);
            assert_eq!(result.1["error"]["code"], "forbidden");
            assert_eq!(
                call(router, "GET", "/api/v1/users", json!({}), credentials)
                    .await
                    .0,
                StatusCode::OK
            );
        }
    }
    let after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM volund.security_audit_events WHERE action='user.update'",
    )
    .fetch_one(&*db)
    .await
    .unwrap();
    assert_eq!(before, after);
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.users WHERE status='active' AND role IN ('owner','administrator') AND locked_until IS NULL")
        .fetch_one(&*db).await.unwrap();
    assert_eq!(active, 2);
}

async fn assert_private_sessions(
    base: &Router,
    owner: &Router,
    first: &LoginSession,
    second: &LoginSession,
) {
    let inventory = call(base, "GET", "/api/v1/sessions", json!({}), Some(first)).await;
    assert_eq!(inventory.0, StatusCode::OK);
    assert_eq!(inventory.1.as_array().unwrap().len(), 2);
    assert!(
        inventory
            .1
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["id"] == first.session.id || entry["id"] == second.session.id)
    );
    let owner_session = call(owner, "GET", "/api/v1/session", json!({}), None).await;
    assert_eq!(owner_session.0, StatusCode::OK);
    let foreign_id = owner_session.1["sessionId"].as_str().unwrap();
    assert_eq!(
        call(
            base,
            "DELETE",
            &format!("/api/v1/sessions/{foreign_id}"),
            json!({}),
            Some(first)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(owner, "GET", "/api/v1/session", json!({}), None)
            .await
            .0,
        StatusCode::OK
    );
}
