#![cfg(target_os = "linux")]
mod support;

use std::{fs, path::PathBuf, time::Duration};
use volundd::{scanner, source_move};

struct Fixture {
    db: support::TestDatabase,
    base: PathBuf,
    id: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = support::test_database().await?;
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("volund-move-boundary-{suffix}"));
        for directory in ["library/one", "library/two", "other/one", "outside"] {
            fs::create_dir_all(base.join(directory)).unwrap();
        }
        fs::write(
            base.join("library/part.step"),
            b"unchanged original CAD bytes",
        )
        .unwrap();
        fs::write(base.join("other/sentinel.step"), b"foreign library bytes").unwrap();
        scanner::register_root(&db, "probe", "Probe", &base.join("library"))
            .await
            .unwrap();
        scanner::register_root(&db, "other", "Other", &base.join("other"))
            .await
            .unwrap();
        scanner::scan_root(&db, "probe", false).await.unwrap();
        let id = sqlx::query_scalar("SELECT public_id::text FROM volund.source_files")
            .fetch_one(&*db)
            .await
            .unwrap();
        Some(Self { db, base, id })
    }

    async fn assert_catalog(&self, path: &str, moves: i64) {
        let rows: Vec<(String, String, String, bool)> = sqlx::query_as(
            "SELECT source.public_id::text,root.root_key,source.relative_path,
                    source.missing_at IS NULL FROM volund.source_files source
             JOIN volund.library_roots root ON root.id=source.library_root_id",
        )
        .fetch_all(&*self.db)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![(self.id.clone(), "probe".into(), path.into(), true)]
        );
        assert_eq!(
            fs::read(self.base.join("library").join(path)).unwrap(),
            b"unchanged original CAD bytes"
        );
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM volund.source_file_moves),
                    (SELECT count(*) FROM volund.source_cleanup)",
        )
        .fetch_one(&*self.db)
        .await
        .unwrap();
        assert_eq!(counts, (moves, 0));
        assert_eq!(
            fs::read(self.base.join("other/sentinel.step")).unwrap(),
            b"foreign library bytes"
        );
        assert_eq!(fs::read_dir(self.base.join("other")).unwrap().count(), 2);
        assert_eq!(
            fs::read_dir(self.base.join("other/one")).unwrap().count(),
            0
        );
        assert_eq!(fs::read_dir(self.base.join("outside")).unwrap().count(), 0);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[tokio::test]
async fn registered_root_cannot_be_redirected_before_a_move() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let error = scanner::register_root(&f.db, "probe", "Replacement", &f.base.join("other"))
        .await
        .unwrap_err();
    assert!(error.contains("already assigned to another filesystem path"));
    let root: (String, String) = sqlx::query_as(
        "SELECT display_name,filesystem_path FROM volund.library_roots WHERE root_key='probe'",
    )
    .fetch_one(&*f.db)
    .await
    .unwrap();
    assert_eq!(
        root,
        (
            "Probe".into(),
            f.base.join("library").to_str().unwrap().into()
        )
    );
    f.assert_catalog("part.step", 0).await;
    let moved = source_move::move_file(&f.db, &f.id, "one").await.unwrap();
    assert_eq!(moved.id, f.id);
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_catalog("one/part.step", 1).await;
}

#[tokio::test]
async fn cross_library_and_outside_destinations_are_rejected() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for (alias, target) in [("foreign", "other"), ("escape", "outside")] {
        std::os::unix::fs::symlink(f.base.join(target), f.base.join("library").join(alias))
            .unwrap();
    }
    for destination in [
        "../other/one",
        "/../other/one/",
        "../outside",
        "foreign/one",
        "escape",
    ] {
        let result = source_move::move_file(&f.db, &f.id, destination).await;
        assert!(
            matches!(result, Err(source_move::MoveError::BadRequest(_))),
            "expected rejected destination {destination}: {result:?}"
        );
        f.assert_catalog("part.step", 0).await;
    }
    // Leading slashes are library-relative UI notation, never a host root switch.
    // A matching local directory makes this assertion independent of ENOENT.
    let absolute = f.base.join("other/one").to_str().unwrap().to_owned();
    let relative = absolute.trim_start_matches('/');
    fs::create_dir_all(f.base.join("library").join(relative)).unwrap();
    let moved = source_move::move_file(&f.db, &f.id, &absolute)
        .await
        .unwrap();
    assert_eq!(moved.path, format!("{relative}/part.step"));
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_catalog(&moved.path, 1).await;
}

async fn concurrent_moves(
    f: &Fixture,
    second: &'static str,
) -> Vec<Result<source_move::MoveResult, source_move::MoveError>> {
    let mut barrier = f.db.begin().await.unwrap();
    sqlx::query("SELECT id FROM volund.source_files FOR UPDATE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for destination in ["one", second] {
        let pool = f.db.clone();
        let id = f.id.clone();
        tasks.push(tokio::spawn(async move {
            source_move::move_file(&pool, &id, destination).await
        }));
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()
                 AND wait_event_type='Lock' AND query LIKE 'SELECT source.id, source.public_id%'",
            )
            .fetch_one(&*f.db)
            .await
            .unwrap();
            if waiting == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both moves must reach the source row lock");
    barrier.rollback().await.unwrap();
    let mut results = Vec::new();
    for task in tasks {
        results.push(
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .expect("move must finish without deadlock")
                .unwrap(),
        );
    }
    results
}

#[tokio::test]
async fn parallel_moves_to_different_directories_form_one_consistent_history() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let results = concurrent_moves(&f, "two").await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let audits: Vec<(String, String)> = sqlx::query_as(
        "SELECT previous_relative_path,new_relative_path FROM volund.source_file_moves ORDER BY id",
    )
    .fetch_all(&*f.db)
    .await
    .unwrap();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0].0, "part.step");
    assert_eq!(audits[1].0, audits[0].1);
    assert_ne!(audits[1].1, audits[0].1);
    for moved in results.into_iter().map(Result::unwrap) {
        assert_eq!(moved.id, f.id);
        assert!(audits.contains(&(moved.previous_path, moved.path)));
    }
    let final_path = &audits[1].1;
    for path in ["part.step", "one/part.step", "two/part.step"] {
        assert_eq!(
            f.base.join("library").join(path).exists(),
            path == final_path
        );
    }
    f.assert_catalog(final_path, 2).await;
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_catalog(final_path, 2).await;
}

#[tokio::test]
async fn parallel_moves_to_the_same_directory_have_one_winner() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let results = concurrent_moves(&f, "one").await;
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    assert_eq!(
        results
            .iter()
            .filter(
                |r| matches!(r, Err(source_move::MoveError::BadRequest(message))
        if message == "source file is already in the selected directory")
            )
            .count(),
        1,
        "{results:?}"
    );
    assert!(!f.base.join("library/part.step").exists());
    assert!(!f.base.join("library/two/part.step").exists());
    f.assert_catalog("one/part.step", 1).await;
    scanner::scan_root(&f.db, "probe", false).await.unwrap();
    f.assert_catalog("one/part.step", 1).await;
}
