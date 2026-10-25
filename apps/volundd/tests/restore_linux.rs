#![cfg(target_os = "linux")]
//! Restore rehearsal: a real `pg_dump` of a populated catalog is restored with
//! `deploy/backup/volund-restore` into a freshly created, empty database, which
//! VÖLUND must then accept as ready and serve unchanged.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use volundd::{database, model_catalog, scanner};

mod support;
use support::test_database;

struct Connection {
    host: String,
    port: String,
    user: String,
    password: String,
    database: String,
}

/// Split `postgres://user:password@host:port/database` into libpq parts.
fn connection(url: &str) -> Connection {
    let rest = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .expect("postgres URL");
    let (credentials, location) = rest.split_once('@').expect("credentials in URL");
    let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
    let (host_port, database) = location.split_once('/').expect("database in URL");
    let database = database.split('?').next().unwrap_or(database);
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, "5432"));
    Connection {
        host: host.to_owned(),
        port: port.to_owned(),
        user: user.to_owned(),
        password: password.to_owned(),
        database: database.to_owned(),
    }
}

fn libpq(command: &mut Command, connection: &Connection) -> Output {
    command
        .env("PGHOST", &connection.host)
        .env("PGPORT", &connection.port)
        .env("PGUSER", &connection.user)
        .env("PGPASSWORD", &connection.password)
        .output()
        .expect("run PostgreSQL client")
}

fn restore(dump: &Path, target: &str, connection: &Connection) -> Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/backup/volund-restore");
    libpq(
        Command::new("sh").arg(script).arg(dump).arg(target),
        connection,
    )
}

async fn populate(pool: &PgPool) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let library = std::env::temp_dir().join(format!("volund-restore-{suffix}"));
    fs::create_dir_all(library.join("Bauteile/clip")).expect("create library");
    fs::write(
        library.join("Bauteile/clip/clip.step"),
        b"ISO-10303-21; clip",
    )
    .expect("write STEP");
    scanner::register_root(pool, "restore_test", "Restore Test", &library)
        .await
        .expect("register root");
    scanner::scan_root(pool, "restore_test", false)
        .await
        .expect("scan fixture");
    sqlx::query(
        "WITH model AS (INSERT INTO volund.models (slug,name,kind,description,license_kind,license_value) \
           VALUES ('clip','Clip','part','Restored metadata','spdx','MIT') RETURNING id) \
         INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal,caption) \
         SELECT model.id,source.id,'master-cad',true,1,'Restored caption' FROM model, volund.source_files source",
    )
    .execute(pool)
    .await
    .expect("create model");
    library
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One rehearsal shares dump, target database and cleanup.
async fn backup_restores_into_an_empty_database_that_volund_accepts() {
    let Some(pool) = test_database().await else {
        return;
    };
    if Command::new("pg_dump").arg("--version").output().is_err() {
        eprintln!("skipping restore rehearsal: PostgreSQL client tools are not installed");
        return;
    }
    let url = std::env::var("VOLUND_TEST_DATABASE_URL").expect("test database URL");
    let source = connection(&url);
    let library = populate(&pool).await;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("volund-restore-dump-{suffix}"));
    fs::create_dir_all(&directory).expect("create dump directory");
    let name = "volund-20261025T000000Z.dump";
    let dump = directory.join(name);
    let dumped = libpq(
        Command::new("pg_dump")
            .arg("--dbname")
            .arg(&source.database)
            .arg("--format=custom")
            .arg("--file")
            .arg(&dump),
        &source,
    );
    assert!(
        dumped.status.success(),
        "{}",
        String::from_utf8_lossy(&dumped.stderr)
    );
    let digest = format!("{:x}", Sha256::digest(fs::read(&dump).expect("read dump")));
    let manifest = directory.join("volund-20261025T000000Z.sha256");
    fs::write(&manifest, format!("{digest}  {name}\n")).expect("write manifest");

    let target = format!(
        "volund_restore_{}_{}",
        std::process::id(),
        suffix % 1_000_000
    );
    sqlx::query(&format!("CREATE DATABASE {target}"))
        .execute(&*pool)
        .await
        .expect("create empty target");

    let restored = restore(&dump, &target, &source);
    let stdout = String::from_utf8_lossy(&restored.stdout);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert!(
        stdout.contains("\"event\":\"restore.completed\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"models\":1"), "{stdout}");
    assert!(
        !stdout.contains(directory.to_str().expect("path")),
        "log must not leak paths"
    );

    let target_url = format!(
        "postgres://{}:{}@{}:{}/{target}",
        source.user, source.password, source.host, source.port
    );
    let restored_pool = PgPool::connect(&target_url)
        .await
        .expect("connect restored");
    let health = database::inspect(&restored_pool)
        .await
        .expect("inspect restored");
    assert!(
        health.is_ready(),
        "restored schema must be ready: {health:?}"
    );
    let original = serde_json::to_value(model_catalog::list_models(&pool).await.expect("models"))
        .expect("serialize models");
    let copy = serde_json::to_value(
        model_catalog::list_models(&restored_pool)
            .await
            .expect("restored models"),
    )
    .expect("serialize restored models");
    assert_eq!(original, copy, "restored catalog must be identical");
    let caption: String =
        sqlx::query_scalar("SELECT caption FROM volund.model_source_files LIMIT 1")
            .fetch_one(&restored_pool)
            .await
            .expect("restored file metadata");
    assert_eq!(caption, "Restored caption");
    restored_pool.close().await;

    // The restore never touches a database that already holds objects.
    let again = restore(&dump, &target, &source);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("restore_target_not_empty"));

    // A dump that no longer matches its manifest is refused before any write.
    fs::write(&manifest, format!("{}  {name}\n", "0".repeat(64))).expect("corrupt manifest");
    let tampered = restore(&dump, &target, &source);
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("restore_checksum_failed"));
    let invalid = restore(&dump, "bad-name;", &source);
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("restore_target_invalid"));

    sqlx::query(&format!("DROP DATABASE {target} WITH (FORCE)"))
        .execute(&*pool)
        .await
        .expect("drop restored database");
    fs::remove_dir_all(directory).expect("remove dump");
    fs::remove_dir_all(library).expect("remove library");
}
