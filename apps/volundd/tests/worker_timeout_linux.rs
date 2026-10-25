#![cfg(target_os = "linux")]
//! Conversion time limits: waiting for the host-wide conversion lock does not
//! count against a job's timeout, and custom mesh deflections reach the
//! converter.
mod support;

use std::fs;

use support::conversion_worker::Fixture;
use tokio_util::sync::CancellationToken;
use volundd::preview_pipeline;

#[tokio::test]
async fn jobs_waiting_for_the_conversion_lock_are_not_timed_out() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let actor: i64 = sqlx::query_scalar("SELECT id FROM volund.users LIMIT 1")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO volund.instance_settings (setting_key,value_json,updated_by_user_id) \
         VALUES ('jobs.conversionConcurrency',to_jsonb(2),$1)",
    )
    .bind(actor)
    .execute(&*f.db)
    .await
    .unwrap();
    let source: String = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
        .fetch_one(&*f.db)
        .await
        .unwrap();
    let second = preview_pipeline::enqueue(&f.db, &source, "fine")
        .await
        .unwrap()
        .id;
    // Each conversion takes 2.5 s of a 3 s limit. The second one waits for the
    // first one's lock, so the old spawn-time deadline (limit + 1 s) failed it.
    let reports =
        preview_pipeline::process_batch(&f.db, &f.config("pause", 3), &CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(reports.len(), 2);
    assert!(
        reports.iter().all(|report| report.status == "ready"),
        "{reports:?}"
    );
    f.assert_terminal(&f.job, "ready", 5).await;
    f.assert_terminal(&second, "ready", 5).await;
}

#[tokio::test]
async fn custom_profile_deflections_reach_the_converter() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query(
        "UPDATE volund.conversion_runs SET profile_snapshot = profile_snapshot \
         || '{\"linearDeflection\":0.05,\"angularDeflection\":0.3}'::jsonb WHERE public_id::text=$1",
    )
    .bind(&f.job)
    .execute(&*f.db)
    .await
    .unwrap();
    let report = preview_pipeline::process_next(&f.db, &f.config("ready", 10))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.status, "ready");
    let arguments = fs::read_to_string(f.root.join("args")).unwrap();
    assert!(
        arguments.contains("--linear-deflection 0.05 --angular-deflection 0.3"),
        "{arguments}"
    );
}
