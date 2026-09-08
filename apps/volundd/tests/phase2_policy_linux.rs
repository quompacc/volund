#![cfg(target_os = "linux")]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use volundd::{api, preview_pipeline, retention, scan_pipeline, scheduler};

mod support;
use support::{authenticated_router, get_json, test_database};

#[tokio::test]
async fn schedules_profiles_and_retention_are_durable_bounded_and_safe() {
    let Some(pool) = test_database().await else {
        return;
    };
    let public = api::router((*pool).clone());
    for path in [
        "/api/v1/scan-schedules",
        "/api/v1/conversion-profiles",
        "/api/v1/retention/preview",
    ] {
        assert_eq!(get_json(&public, path).await.0, StatusCode::UNAUTHORIZED);
    }
    let router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-policy-{suffix}"));
    let derived = root.join("derived");
    let library = root.join("library");
    fs::create_dir_all(&derived).unwrap();
    fs::create_dir_all(&library).unwrap();
    let library_id:String=sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ($1,'Policy library',$2) RETURNING public_id::text").bind(format!("policy_{suffix}")).bind(library.to_string_lossy().as_ref()).fetch_one(&*pool).await.unwrap();

    assert_eq!(request(&router,"POST","/api/v1/conversion-profiles",json!({"name":"production-fine","nativePreset":"fine","linearDeflection":0.02,"angularDeflection":0.2,"enabled":true,"confirmation":"wrong"})).await.0, StatusCode::BAD_REQUEST);
    let (status,profile)=request(&router,"POST","/api/v1/conversion-profiles",json!({"name":"production-fine","nativePreset":"fine","linearDeflection":0.02,"angularDeflection":0.2,"enabled":true,"confirmation":"APPLY PROFILE new"})).await;
    assert_eq!(status, StatusCode::CREATED);
    let profile_id = profile["id"].as_str().unwrap();
    let source_id = source_fixture(&pool, &library_id, &library).await;
    preview_pipeline::enqueue(&pool, &source_id, profile_id)
        .await
        .unwrap();
    let snapshot:Value=sqlx::query_scalar("SELECT profile_snapshot FROM volund.conversion_runs WHERE conversion_profile_id=(SELECT id FROM volund.conversion_profiles WHERE public_id::text=$1)").bind(profile_id).fetch_one(&*pool).await.unwrap();
    assert_eq!(snapshot["nativePreset"], "fine");
    assert_eq!(snapshot["linearDeflection"], 0.02);
    let update = json!({"expectedRevision":1,"name":"production-fine","nativePreset":"web","linearDeflection":null,"angularDeflection":null,"enabled":true,"confirmation":format!("APPLY PROFILE {profile_id}")});
    assert_eq!(
        request(
            &router,
            "PUT",
            &format!("/api/v1/conversion-profiles/{profile_id}"),
            update
        )
        .await
        .0,
        StatusCode::OK
    );
    let unchanged: Value =
        sqlx::query_scalar("SELECT profile_snapshot FROM volund.conversion_runs LIMIT 1")
            .fetch_one(&*pool)
            .await
            .unwrap();
    assert_eq!(unchanged["nativePreset"], "fine");

    assert_retired_profile(&pool, &router, profile_id, &source_id, &snapshot).await;

    let (status,schedule)=request(&router,"POST","/api/v1/scan-schedules",json!({"libraryId":library_id,"name":"nightly","localTime":"02:30","timeZone":"Europe/Berlin","weekdayMask":127,"fullScan":false,"enabled":true,"confirmation":"APPLY SCHEDULE new"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(schedule["nextRunAtUnixMs"].as_i64().is_some());
    sqlx::query("UPDATE volund.scan_schedules SET next_run_at=now()-interval '3 days'")
        .execute(&*pool)
        .await
        .unwrap();
    assert_eq!(scheduler::process_due(&pool).await.unwrap(), 1);
    assert_eq!(scheduler::process_due(&pool).await.unwrap(), 0);
    let scheduled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.scan_runs WHERE schedule_id IS NOT NULL")
            .fetch_one(&*pool)
            .await
            .unwrap();
    assert_eq!(scheduled, 1);
    let schedule_id = schedule["id"].as_str().unwrap();
    assert_eq!(
        request(
            &router,
            "DELETE",
            &format!("/api/v1/scan-schedules/{schedule_id}"),
            json!({"confirmation":"wrong","expectedRevision":1})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    assert_deleted_schedule(&pool, &router, schedule_id).await;

    retention_fixture(&pool, &derived).await;
    sqlx::query("UPDATE volund.conversion_runs SET artifact_protected_until=now()+interval '10 minutes' WHERE status='ready'").execute(&*pool).await.unwrap();
    assert_eq!(retention::preview(&pool).await.unwrap().artifact_files, 0);
    sqlx::query("UPDATE volund.conversion_runs SET artifact_protected_until=now()-interval '1 second' WHERE status='ready'").execute(&*pool).await.unwrap();
    let preview = retention::preview(&pool).await.unwrap();
    assert_eq!(preview.artifact_files, 1);
    let original = library.join("part.step");
    let before = fs::read(&original).unwrap();
    let result = retention::execute(&pool, &derived, None, None)
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(fs::read(&original).unwrap(), before);
    let artifact_count: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.derived_artifacts")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(artifact_count, 0);
    assert_policy_admin_only(&pool, &router).await;
    fs::remove_dir_all(root).unwrap();
}

async fn assert_retired_profile(
    pool: &sqlx::PgPool,
    router: &axum::Router,
    profile_id: &str,
    source_id: &str,
    snapshot: &Value,
) {
    let retirement = json!({"confirmation":format!("RETIRE {profile_id}"),"expectedRevision":2});
    let profile_path = format!("/api/v1/conversion-profiles/{profile_id}");
    let (status, retired) = request(router, "DELETE", &profile_path, retirement.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retired["enabled"], false);
    assert_eq!(
        request(router, "DELETE", &profile_path, retirement).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(
        preview_pipeline::enqueue(pool, source_id, profile_id)
            .await
            .is_err()
    );
    let preserved: Value =
        sqlx::query_scalar("SELECT profile_snapshot FROM volund.conversion_runs LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(&preserved, snapshot);
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.conversion_runs WHERE status='queued'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(queued, 1);
    let retire_audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='conversion-profile.retire' AND outcome='success'").fetch_one(pool).await.unwrap();
    assert_eq!(retire_audits, 1);
}

async fn assert_deleted_schedule(pool: &sqlx::PgPool, router: &axum::Router, schedule_id: &str) {
    let scan_before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s)-'schedule_id' FROM volund.scan_runs s WHERE schedule_id IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let deletion =
        json!({"confirmation":format!("DELETE SCHEDULE {schedule_id}"),"expectedRevision":1});
    let schedule_path = format!("/api/v1/scan-schedules/{schedule_id}");
    assert_eq!(
        request(router, "DELETE", &schedule_path, deletion.clone())
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(router, "DELETE", &schedule_path, deletion).await.0,
        StatusCode::BAD_REQUEST
    );
    let scan_after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s)-'schedule_id' FROM volund.scan_runs s WHERE public_id::text=$1",
    )
    .bind(scan_before["public_id"].as_str().unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(scan_before, scan_after);
    let detached: bool = sqlx::query_scalar(
        "SELECT schedule_id IS NULL FROM volund.scan_runs WHERE public_id::text=$1",
    )
    .bind(scan_before["public_id"].as_str().unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(detached);
    assert_eq!(scheduler::process_due(pool).await.unwrap(), 0);
    let delete_audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.security_audit_events WHERE action='scan-schedule.delete' AND outcome='success'").fetch_one(pool).await.unwrap();
    assert_eq!(delete_audits, 1);
}

#[tokio::test]
async fn retention_refuses_symlinks_and_keeps_the_catalog_entry() {
    use std::os::unix::fs::symlink;
    let Some(pool) = test_database().await else {
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "volund-retention-link-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let derived = root.join("derived");
    fs::create_dir_all(&derived).unwrap();
    retention_fixture(&pool, &derived).await;
    let outside = root.join("outside.glb");
    fs::write(&outside, b"original").unwrap();
    fs::remove_file(derived.join("old.glb")).unwrap();
    symlink(&outside, derived.join("old.glb")).unwrap();
    let result = retention::execute(&pool, &derived, None, None)
        .await
        .unwrap();
    assert_eq!(result.status, "partial");
    assert!(
        result
            .result_codes
            .contains(&"artifact_symlink_refused".to_owned())
    );
    assert_eq!(fs::read(&outside).unwrap(), b"original");
    let artifacts: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.derived_artifacts")
        .fetch_one(&*pool)
        .await
        .unwrap();
    assert_eq!(artifacts, 1);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn retention_count_limit_is_scoped_to_each_content_object() {
    let Some(pool) = test_database().await else {
        return;
    };
    let _router = authenticated_router(&pool, api::router((*pool).clone())).await;
    sqlx::query("INSERT INTO volund.instance_settings (setting_key,value_json,updated_by_user_id) SELECT 'retention.maxArtifactRuns','10'::jsonb,id FROM volund.users LIMIT 1")
        .execute(&*pool).await.unwrap();
    let root = std::env::temp_dir().join(format!(
        "volund-retention-count-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    // Eleven distinct contents, each with only one recent preview, must all survive.
    let mut first_content = 0_i64;
    for index in 0..11 {
        let content: i64 = sqlx::query_scalar(
            "INSERT INTO volund.content_objects (sha256,byte_size) VALUES ($1,1) RETURNING id",
        )
        .bind(format!("{index:064x}"))
        .fetch_one(&*pool)
        .await
        .unwrap();
        if index == 0 {
            first_content = content;
        }
        recent_retention_run(&pool, &root, content, &format!("source-{index}.glb")).await;
    }
    assert_eq!(retention::preview(&pool).await.unwrap().artifact_runs, 0);
    assert_eq!(
        retention::execute(&pool, &root, None, None)
            .await
            .unwrap()
            .artifact_runs,
        0
    );
    // Add ten versions to one content; only its oldest version exceeds the limit.
    for index in 0..10 {
        recent_retention_run(&pool, &root, first_content, &format!("extra-{index}.glb")).await;
    }
    assert_eq!(retention::preview(&pool).await.unwrap().artifact_runs, 1);
    let result = retention::execute(&pool, &root, None, None).await.unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.artifact_runs, 1);
    assert!(!root.join("source-0.glb").exists());
    for index in 1..11 {
        assert!(root.join(format!("source-{index}.glb")).exists());
    }
    assert_eq!(retention::preview(&pool).await.unwrap().artifact_runs, 0);
    fs::remove_dir_all(root).unwrap();
}

async fn recent_retention_run(
    pool: &sqlx::PgPool,
    root: &std::path::Path,
    content: i64,
    path: &str,
) {
    let run: i64 = sqlx::query_scalar("INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,contract_version,profile,status,started_at,finished_at) VALUES ($1,'volund-cad-convert',$2,1,'web','ready',now(),now()) RETURNING id")
        .bind(content).bind(path).fetch_one(pool).await.unwrap();
    fs::write(root.join(path), b"preview").unwrap();
    sqlx::query("INSERT INTO volund.derived_artifacts (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type) VALUES ($1,'preview-glb',$2,$3,7,'model/gltf-binary')")
        .bind(run).bind(path).bind("0".repeat(64)).execute(pool).await.unwrap();
}

#[tokio::test]
async fn reduced_limits_do_not_interrupt_running_work_or_claim_more() {
    let Some(pool) = test_database().await else {
        return;
    };
    let _router = authenticated_router(&pool, api::router((*pool).clone())).await;
    let actor: i64 = sqlx::query_scalar("SELECT id FROM volund.users LIMIT 1")
        .fetch_one(&*pool)
        .await
        .unwrap();
    for (key, value) in [
        ("jobs.scanConcurrency", 1),
        ("jobs.conversionConcurrency", 1),
    ] {
        sqlx::query("INSERT INTO volund.instance_settings (setting_key,value_json,updated_by_user_id) VALUES ($1,to_jsonb($2::int),$3)").bind(key).bind(value).bind(actor).execute(&*pool).await.unwrap();
    }
    let root = std::env::temp_dir().join("volund-concurrency-limit");
    fs::create_dir_all(&root).unwrap();
    for index in 0..2 {
        let library:i64=sqlx::query_scalar("INSERT INTO volund.library_roots (root_key,display_name,filesystem_path) VALUES ($1,$1,$2) RETURNING id").bind(format!("limit_{index}")).bind(root.join(index.to_string()).to_string_lossy().as_ref()).fetch_one(&*pool).await.unwrap();
        sqlx::query("INSERT INTO volund.scan_runs (library_root_id,status) VALUES ($1,$2)")
            .bind(library)
            .bind(if index == 0 { "running" } else { "queued" })
            .execute(&*pool)
            .await
            .unwrap();
    }
    assert!(scan_pipeline::process_next(&pool).await.unwrap().is_none());
    let states: Vec<String> = sqlx::query_scalar("SELECT status FROM volund.scan_runs ORDER BY id")
        .fetch_all(&*pool)
        .await
        .unwrap();
    assert_eq!(states, vec!["running", "queued"]);
    let profile: i64 =
        sqlx::query_scalar("SELECT id FROM volund.conversion_profiles WHERE name='web'")
            .fetch_one(&*pool)
            .await
            .unwrap();
    for index in 0..2 {
        let content: i64 = sqlx::query_scalar(
            "INSERT INTO volund.content_objects (sha256,byte_size) VALUES ($1,1) RETURNING id",
        )
        .bind(format!("{index:064x}"))
        .fetch_one(&*pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,contract_version,profile,status,started_at,conversion_profile_id,conversion_profile_revision,profile_snapshot) VALUES ($1,'volund-cad-convert','test',1,'web',$2,CASE WHEN $2='running' THEN now() END,$3,1,'{\"nativePreset\":\"web\"}')").bind(content).bind(if index==0{"running"}else{"queued"}).bind(profile).execute(&*pool).await.unwrap();
    }
    let config = preview_pipeline::PreviewWorkerConfig::new(
        "/bin/false".into(),
        "/bin/false".into(),
        "/var/tmp/derived".into(),
        "/var/tmp/scratch".into(),
        1,
    )
    .unwrap();
    assert!(
        preview_pipeline::process_next(&pool, &config)
            .await
            .unwrap()
            .is_none()
    );
    fs::remove_dir_all(root).unwrap();
}

async fn source_fixture(
    pool: &sqlx::PgPool,
    library_id: &str,
    library: &std::path::Path,
) -> String {
    let root_id: i64 =
        sqlx::query_scalar("SELECT id FROM volund.library_roots WHERE public_id::text=$1")
            .bind(library_id)
            .fetch_one(pool)
            .await
            .unwrap();
    fs::write(library.join("part.step"), b"ISO-10303-21;").unwrap();
    let scan:i64=sqlx::query_scalar("INSERT INTO volund.scan_runs (library_root_id,status,finished_at) VALUES ($1,'completed',now()) RETURNING id").bind(root_id).fetch_one(pool).await.unwrap();
    let content:i64=sqlx::query_scalar("INSERT INTO volund.content_objects (sha256,byte_size,detected_format) VALUES ($1,13,'step') RETURNING id").bind(format!("{root_id:064x}")).fetch_one(pool).await.unwrap();
    sqlx::query_scalar("INSERT INTO volund.source_files (library_root_id,content_object_id,relative_path,filesystem_modified_at,last_seen_scan_id) VALUES ($1,$2,'part.step',now(),$3) RETURNING public_id::text").bind(root_id).bind(content).bind(scan).fetch_one(pool).await.unwrap()
}

async fn assert_policy_admin_only(pool: &sqlx::PgPool, router: &axum::Router) {
    for role in ["editor", "viewer"] {
        sqlx::query("UPDATE volund.users SET role=$1")
            .bind(role)
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            get_json(router, "/api/v1/conversion-profiles").await.0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE volund.users SET role='owner'")
        .execute(pool)
        .await
        .unwrap();
}

async fn retention_fixture(pool: &sqlx::PgPool, derived: &std::path::Path) {
    let content:i64=sqlx::query_scalar("INSERT INTO volund.content_objects (sha256,byte_size,detected_format) VALUES ($1,1,'step') RETURNING id").bind(format!("{:064x}",99991)).fetch_one(pool).await.unwrap();
    let profile: i64 =
        sqlx::query_scalar("SELECT id FROM volund.conversion_profiles WHERE name='web'")
            .fetch_one(pool)
            .await
            .unwrap();
    let run:i64=sqlx::query_scalar("INSERT INTO volund.conversion_runs (content_object_id,converter_name,converter_version,contract_version,profile,status,requested_at,started_at,finished_at,conversion_profile_id,conversion_profile_revision,profile_snapshot,diagnostics) VALUES ($1,'volund-cad-convert','test',1,'web','ready',now()-interval '200 days',now()-interval '200 days',now()-interval '200 days',$2,1,'{\"nativePreset\":\"web\"}','[{\"message\":\"old\"}]') RETURNING id").bind(content).bind(profile).fetch_one(pool).await.unwrap();
    fs::write(derived.join("old.glb"), b"old").unwrap();
    sqlx::query("INSERT INTO volund.derived_artifacts (conversion_run_id,artifact_kind,relative_path,sha256,byte_size,media_type,created_at) VALUES ($1,'preview-glb','old.glb',$2,3,'model/gltf-binary',now()-interval '200 days')").bind(run).bind("0".repeat(64)).execute(pool).await.unwrap();
}

async fn request(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}
