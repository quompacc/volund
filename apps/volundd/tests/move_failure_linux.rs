#![cfg(target_os = "linux")]
mod support;

async fn fixture() -> Option<(support::TestDatabase, std::path::PathBuf, String)> {
    let db = support::test_database().await?;
    let root = std::env::temp_dir().join(format!(
        "volund-move-review-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("part.step"), b"AAAA").unwrap();
    volundd::scanner::register_root(&db, "probe", "Probe", &root)
        .await
        .unwrap();
    volundd::scanner::scan_root(&db, "probe", false)
        .await
        .unwrap();
    let id = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
        .fetch_one(&*db)
        .await
        .unwrap();
    Some((db, root, id))
}

#[tokio::test]
async fn cancelled_move_preserves_original_and_can_retry() {
    let Some((db, root, id)) = fixture().await else {
        return;
    };
    let mut barrier = db.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(674840)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION volund.pause_move_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(674840); RETURN NEW; END $$").execute(&*db).await.unwrap();
    sqlx::query("CREATE TRIGGER pause_move_probe BEFORE INSERT ON volund.source_file_moves FOR EACH ROW EXECUTE FUNCTION volund.pause_move_probe()").execute(&*db).await.unwrap();
    let pool = db.clone();
    let move_id = id.clone();
    let task =
        tokio::spawn(
            async move { volundd::source_move::move_file(&pool, &move_id, "target").await },
        );
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objid=674840 AND NOT granted)").fetch_one(&*db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    sqlx::query("SELECT pg_advisory_unlock(674840)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    drop(barrier);
    sqlx::query("DROP TRIGGER pause_move_probe ON volund.source_file_moves")
        .execute(&*db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.pause_move_probe()")
        .execute(&*db)
        .await
        .unwrap();
    let path: String = sqlx::query_scalar("SELECT relative_path FROM volund.source_files")
        .fetch_one(&*db)
        .await
        .unwrap();
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.source_file_moves")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert_eq!(path, "part.step");
    assert_eq!(std::fs::read(root.join(&path)).unwrap(), b"AAAA");
    assert!(!root.join("target/part.step").exists());

    assert_eq!(audits, 0);
    let moved = volundd::source_move::move_file(&db, &id, "target")
        .await
        .unwrap();
    assert_eq!(moved.id, id);
    assert!(!root.join("part.step").exists());
    assert_eq!(std::fs::read(root.join(&moved.path)).unwrap(), b"AAAA");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
            .fetch_one(&*db)
            .await
            .unwrap(),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn move_into_internal_quarantine_is_rejected() {
    let Some((db, root, id)) = fixture().await else {
        return;
    };
    std::fs::create_dir(root.join(".volund-quarantine")).unwrap();
    for destination in [
        ".volund-quarantine",
        ".volund-quarantine/nested",
        "/.volund-quarantine/",
    ] {
        assert!(matches!(
            volundd::source_move::move_file(&db, &id, destination).await,
            Err(volundd::source_move::MoveError::BadRequest(_))
        ));
    }
    let report = volundd::scanner::scan_root(&db, "probe", false)
        .await
        .unwrap();
    let missing: bool = sqlx::query_scalar(
        "SELECT missing_at IS NOT NULL FROM volund.source_files WHERE public_id::text=$1",
    )
    .bind(&id)
    .fetch_one(&*db)
    .await
    .unwrap();
    let quarantines: i64 = sqlx::query_scalar("SELECT count(*) FROM volund.source_quarantines")
        .fetch_one(&*db)
        .await
        .unwrap();
    assert!(!missing);
    assert_eq!(report.discovered_files, 1);
    assert_eq!(quarantines, 0);
    assert_eq!(std::fs::read(root.join("part.step")).unwrap(), b"AAAA");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn move_audit_failure_rolls_back_staging_and_changed_bytes_are_rejected() {
    let Some((db, root, id)) = fixture().await else {
        return;
    };
    sqlx::query("CREATE FUNCTION volund.fail_move_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test move audit failure'; END $$").execute(&*db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_move_probe BEFORE INSERT ON volund.source_file_moves FOR EACH ROW EXECUTE FUNCTION volund.fail_move_probe()").execute(&*db).await.unwrap();
    assert!(
        volundd::source_move::move_file(&db, &id, "target")
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER fail_move_probe ON volund.source_file_moves")
        .execute(&*db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.fail_move_probe()")
        .execute(&*db)
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.join("part.step")).unwrap(), b"AAAA");
    assert!(!root.join("target/part.step").exists());
    std::fs::write(root.join("part.step"), b"BBBB").unwrap();
    assert!(matches!(
        volundd::source_move::move_file(&db, &id, "target").await,
        Err(volundd::source_move::MoveError::Conflict(_))
    ));
    assert!(!root.join("target/part.step").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn move_cleanup_retries_after_committed_unlink_failure() {
    let Some((db, root, id)) = fixture().await else {
        return;
    };
    sqlx::query("CREATE FUNCTION volund.fail_cleanup_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test cleanup acknowledgement failure'; END $$").execute(&*db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_cleanup_probe BEFORE DELETE ON volund.source_cleanup FOR EACH ROW EXECUTE FUNCTION volund.fail_cleanup_probe()").execute(&*db).await.unwrap();
    assert!(matches!(
        volundd::source_move::move_file(&db, &id, "target").await,
        Err(volundd::source_move::MoveError::Storage(_))
    ));
    sqlx::query("DROP TRIGGER fail_cleanup_probe ON volund.source_cleanup")
        .execute(&*db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.fail_cleanup_probe()")
        .execute(&*db)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(root.join("target/part.step")).unwrap(),
        b"AAAA"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT relative_path FROM volund.source_files")
            .fetch_one(&*db)
            .await
            .unwrap(),
        "target/part.step"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
            .fetch_one(&*db)
            .await
            .unwrap(),
        1
    );
    // A subsequent move finishes the pending intent before staging another one.
    volundd::source_move::move_file(&db, &id, "").await.unwrap();
    assert_eq!(std::fs::read(root.join("part.step")).unwrap(), b"AAAA");
    assert!(!root.join("target/part.step").exists());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM volund.source_cleanup")
            .fetch_one(&*db)
            .await
            .unwrap(),
        0
    );
    volundd::scanner::scan_root(&db, "probe", false)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT public_id::text FROM volund.source_files WHERE missing_at IS NULL"
        )
        .fetch_one(&*db)
        .await
        .unwrap(),
        id
    );
    std::fs::remove_dir_all(root).unwrap();
}
