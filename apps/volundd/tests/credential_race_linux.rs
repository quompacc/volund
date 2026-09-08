#![cfg(target_os = "linux")]
mod support;

use std::time::Duration;
use volundd::session::{self, LoginInput, SessionError, SessionPolicy};
use volundd::user_admin;

const OLD: &str = "catalog test password with enough entropy";
const NEW: &str = "new credential after serialized replacement";

#[tokio::test]
async fn credential_replacement_rejects_in_flight_old_password_operations() {
    for (reset_first, change_second) in [(true, false), (false, false), (true, true)] {
        let Some(db) = support::test_database().await else {
            return;
        };
        let _router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
        let initial = session::login(&db, input(OLD), SessionPolicy::default())
            .await
            .unwrap();
        let actor = session::authenticate(&db, &initial.session_token, SessionPolicy::default())
            .await
            .unwrap();
        let mut barrier = db.begin().await.unwrap();
        sqlx::query("SELECT id FROM volund.users FOR UPDATE")
            .fetch_all(&mut *barrier)
            .await
            .unwrap();
        let writer_pool = db.clone();
        let writer_actor = actor.clone();
        let writer = tokio::spawn(async move {
            if reset_first {
                user_admin::reset_password(
                    &writer_pool,
                    &writer_actor,
                    &writer_actor.user_id,
                    NEW.to_owned(),
                    false,
                )
                .await
            } else {
                user_admin::change_own_password(
                    &writer_pool,
                    &writer_actor,
                    OLD.to_owned(),
                    NEW.to_owned(),
                )
                .await
            }
        });
        // Observe the actual PostgreSQL lock queue, never guess Argon2 timing.
        wait_for_waiters(&db, 1).await;
        let stale_pool = db.clone();
        let stale_actor = actor.clone();
        let stale = tokio::spawn(async move {
            if change_second {
                user_admin::change_own_password(
                    &stale_pool,
                    &stale_actor,
                    OLD.to_owned(),
                    "stale replacement must never win".to_owned(),
                )
                .await
                .is_err()
            } else {
                matches!(
                    session::login(&stale_pool, input(OLD), SessionPolicy::default()).await,
                    Err(SessionError::InvalidCredentials)
                )
            }
        });
        wait_for_waiters(&db, 2).await;
        barrier.commit().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), writer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), stale)
                .await
                .unwrap()
                .unwrap(),
            "old password operation survived replacement"
        );
        assert!(matches!(
            session::login(&db, input(OLD), SessionPolicy::default()).await,
            Err(SessionError::InvalidCredentials)
        ));
        let fresh = session::login(&db, input(NEW), SessionPolicy::default())
            .await
            .unwrap();
        assert!(
            session::authenticate(&db, &fresh.session_token, SessionPolicy::default())
                .await
                .is_ok()
        );
    }
}

async fn wait_for_waiters(pool: &sqlx::PgPool, minimum: i64) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND cardinality(pg_blocking_pids(pid))>0")
                .fetch_one(pool).await.unwrap();
            if count >= minimum { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("operations reached the deterministic row-lock barrier");
}

fn input(password: &str) -> LoginInput {
    LoginInput {
        email: "catalog-owner@example.test".to_owned(),
        password: password.to_owned(),
        client_address: None,
        user_agent: Some("credential-race-test".to_owned()),
    }
}
