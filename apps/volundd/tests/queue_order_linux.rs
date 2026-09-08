#![cfg(target_os = "linux")]
mod support;

use volundd::{preview_pipeline, scan_pipeline};

#[tokio::test]
async fn scans_and_conversions_claim_oldest_request_then_id() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let root = std::env::temp_dir().join(format!("volund-queue-order-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let config = preview_pipeline::PreviewWorkerConfig::new(
        "/bin/false".into(),
        "/bin/false".into(),
        root.join("derived"),
        root.join("scratch"),
        1,
    )
    .unwrap();
    let mut scans = Vec::new();
    let mut conversions = Vec::new();
    for index in 0..3 {
        let library_path = root.join(index.to_string());
        std::fs::create_dir_all(&library_path).unwrap();
        std::fs::write(
            library_path.join("fixture.step"),
            format!("synthetic scan fixture {index}"),
        )
        .unwrap();
        let library: i64 = sqlx::query_scalar("INSERT INTO volund.library_roots(root_key,display_name,filesystem_path) VALUES($1,$1,$2) RETURNING id")
            .bind(format!("queue-{index}")).bind(library_path.to_str().unwrap()).fetch_one(&*db).await.unwrap();
        let scan: String = sqlx::query_scalar("INSERT INTO volund.scan_runs(library_root_id,status,requested_at) VALUES($1,'queued',now()-make_interval(days => $2)) RETURNING public_id::text")
            .bind(library).bind(if index == 0 { 1 } else { 2 }).fetch_one(&*db).await.unwrap();
        scans.push(scan);
        let content: i64 = sqlx::query_scalar("INSERT INTO volund.content_objects(sha256,byte_size,detected_format) VALUES($1,1,'step') RETURNING id")
            .bind(format!("{index:064x}")).fetch_one(&*db).await.unwrap();
        let conversion: String = sqlx::query_scalar("INSERT INTO volund.conversion_runs(content_object_id,converter_name,converter_version,contract_version,profile,status,requested_at) VALUES($1,'volund-cad-convert','test',1,'web','queued',now()-make_interval(days => $2)) RETURNING public_id::text")
            .bind(content).bind(if index == 0 { 1 } else { 2 }).fetch_one(&*db).await.unwrap();
        conversions.push(conversion);
    }
    // Equal timestamps deliberately distinguish the ID tie-break from time order.
    for table in ["scan_runs", "conversion_runs"] {
        sqlx::query(&format!("UPDATE volund.{table} SET requested_at=(SELECT min(requested_at) FROM volund.{table}) WHERE public_id::text IN ($1,$2)"))
            .bind(if table == "scan_runs" { &scans[1] } else { &conversions[1] })
            .bind(if table == "scan_runs" { &scans[2] } else { &conversions[2] })
            .execute(&*db).await.unwrap();
    }
    for index in [1, 2, 0] {
        let scan = scan_pipeline::process_next(&db).await.unwrap().unwrap();
        assert_eq!(scan.id, scans[index]);
        assert_eq!(scan.status, "completed");
        let detail = volundd::job_admin::get(&db, "scan", &scan.id)
            .await
            .unwrap();
        assert_eq!(detail.status, scan.status);
        assert_eq!(
            (detail.progress_current, detail.progress_total),
            (1, Some(1))
        );
        assert!(detail.finished_at_unix_ms.is_some());
        let conversion = preview_pipeline::process_next(&db, &config)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conversion.id, conversions[index]);
        // No source matches the queued content: persist an honest failure.
        assert_eq!(conversion.status, "failed");
        let stored: String = sqlx::query_scalar(
            "SELECT status FROM volund.conversion_runs WHERE public_id::text=$1",
        )
        .bind(&conversion.id)
        .fetch_one(&*db)
        .await
        .unwrap();
        assert_eq!(stored, conversion.status);
        let detail = volundd::job_admin::get(&db, "conversion", &conversion.id)
            .await
            .unwrap();
        assert_eq!(detail.status, "failed");
        assert!(detail.diagnostic.is_some());
        assert!(detail.can_retry);
    }
    assert!(scan_pipeline::process_next(&db).await.unwrap().is_none());
    assert!(
        preview_pipeline::process_next(&db, &config)
            .await
            .unwrap()
            .is_none()
    );
    std::fs::remove_dir_all(root).unwrap();
}
