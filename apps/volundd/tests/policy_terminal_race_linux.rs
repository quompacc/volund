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
async fn profile_retirement_serializes_with_edits() {
    for terminal_first in [true, false] {
        race(false, terminal_first).await;
    }
}
#[tokio::test]
async fn schedule_deletion_serializes_with_edits() {
    for terminal_first in [true, false] {
        race(true, terminal_first).await;
    }
}

async fn wait_for_writers(db: &sqlx::PgPool, minimum: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND wait_event IN ('relation','transactionid','tuple') AND cardinality(pg_blocking_pids(pid))>0").fetch_one(db).await.unwrap();
            if waiting >= minimum { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
}

async fn race(schedule: bool, terminal_first: bool) {
    let Some(db) = support::test_database().await else {
        return;
    };
    let router = support::authenticated_router(&db, volundd::api::router(db.clone())).await;
    let (route, table, prefix, mut input) = if schedule {
        let root: String = sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ('terminal','Terminal','/var/tmp/unused-terminal-race') RETURNING public_id::text").fetch_one(&*db).await.unwrap();
        (
            "scan-schedules",
            "scan_schedules",
            "scan-schedule",
            json!({"libraryId":root,"name":"Before","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true,"confirmation":"APPLY SCHEDULE new"}),
        )
    } else {
        (
            "conversion-profiles",
            "conversion_profiles",
            "conversion-profile",
            json!({"name":"Before","nativePreset":"web","linearDeflection":null,"angularDeflection":null,"enabled":true,"confirmation":"APPLY PROFILE new"}),
        )
    };
    let list_path = format!("/api/v1/{route}");
    let (status, created) = request(&router, "POST", &list_path, input.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    let path = format!("{list_path}/{id}");
    input["expectedRevision"] = created["revision"].clone();
    input["name"] = json!("After");
    input["confirmation"] = json!(format!(
        "APPLY {} {id}",
        if schedule { "SCHEDULE" } else { "PROFILE" }
    ));
    let terminal = json!({"confirmation":format!("{} {id}",if schedule {"DELETE SCHEDULE"} else {"RETIRE"}),"expectedRevision":created["revision"]});
    let mut barrier = db.begin().await.unwrap();
    sqlx::query(&format!(
        "SELECT id FROM volund.{table} WHERE public_id::text=$1 FOR UPDATE"
    ))
    .bind(id)
    .execute(&mut *barrier)
    .await
    .unwrap();
    let mut tasks = Vec::new();
    for is_terminal in [terminal_first, !terminal_first] {
        let r = router.clone();
        let p = path.clone();
        let body = if is_terminal {
            terminal.clone()
        } else {
            input.clone()
        };
        tasks.push((
            is_terminal,
            tokio::spawn(async move {
                request(&r, if is_terminal { "DELETE" } else { "PUT" }, &p, body).await
            }),
        ));
        wait_for_writers(&db, i64::try_from(tasks.len()).unwrap()).await;
    }
    barrier.rollback().await.unwrap();
    let mut edited = false;
    let mut terminated = false;
    for (is_terminal, task) in tasks {
        let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        let success = if is_terminal && schedule {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::OK
        };
        assert!(
            status == success || status == StatusCode::BAD_REQUEST,
            "schedule={schedule} terminal_first={terminal_first}: {body}"
        );
        if is_terminal {
            terminated = status == success;
        } else {
            edited = status == success;
        }
    }
    assert_ne!(
        edited, terminated,
        "exactly one revision-bound mutation must win"
    );
    let (_, listed) = request(&router, "GET", &list_path, json!({})).await;
    let target = listed.as_array().unwrap().iter().find(|v| v["id"] == id);
    if schedule && terminated {
        assert!(target.is_none());
    } else {
        let target = target.unwrap();
        assert_eq!(target["enabled"], edited);
        assert_eq!(target["name"], if edited { "After" } else { "Before" });
        assert_eq!(target["revision"], 2);
    }
    assert_audits(&db, prefix, id, schedule, edited).await;
}

async fn assert_audits(db: &sqlx::PgPool, prefix: &str, id: &str, schedule: bool, edited: bool) {
    for (suffix, expected) in [
        ("update", i64::from(edited)),
        (
            if schedule { "delete" } else { "retire" },
            i64::from(!edited),
        ),
    ] {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action=$1 AND target_public_id::text=$2 AND outcome='success'").bind(format!("{prefix}.{suffix}")).bind(id).fetch_one(db).await.unwrap();
        assert_eq!(count, expected);
    }
}
