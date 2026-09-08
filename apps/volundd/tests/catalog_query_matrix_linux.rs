#![cfg(target_os = "linux")]
mod support;

use axum::http::StatusCode;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use support::{authenticated_router, get_json, test_database};
use volundd::{api, scanner};

#[tokio::test]
async fn combined_catalog_queries_keep_unicode_totals_and_stable_pages() {
    let Some(db) = test_database().await else {
        return;
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("volund-catalog-matrix-{suffix}"));
    fs::create_dir_all(root.join("Sonderzeichen")).unwrap();
    fs::write(
        root.join("Sonderzeichen/Übergröße + 100%.step"),
        b"unicode fixture",
    )
    .unwrap();
    for folder in 0..3 {
        fs::create_dir_all(root.join(format!("Ordner-{folder}"))).unwrap();
        fs::write(
            root.join(format!("Ordner-{folder}/eintrag.step")),
            b"folder fixture",
        )
        .unwrap();
    }
    fs::create_dir_all(root.join("bulk")).unwrap();
    for index in 0..120 {
        let extension = ["obj", "step", "stl"][index % 3];
        fs::write(
            root.join(format!("bulk/teil-{index:03}.{extension}")),
            b"same-size",
        )
        .unwrap();
    }
    let root_key = format!("matrix_{}", std::process::id());
    scanner::register_root(&db, &root_key, "Query Matrix", &root)
        .await
        .unwrap();
    scanner::scan_root(&db, &root_key, false).await.unwrap();
    sqlx::query("UPDATE volund.source_files SET filesystem_modified_at='2026-01-01 00:00:00+00' WHERE library_root_id=(SELECT id FROM volund.library_roots WHERE root_key=$1)")
        .bind(&root_key).execute(&*db).await.unwrap();
    let router = authenticated_router(&db, api::router(db.clone())).await;

    assert_query(
        &router,
        &root_key,
        "directory=Sonderzeichen&q=%C3%9CBERGR%C3%B6%C3%9Fe&format=step",
        1,
        &["Sonderzeichen/Übergröße + 100%.step"],
    )
    .await;
    assert_query(
        &router,
        &root_key,
        "directory=Sonderzeichen&q=%2B+100%25&format=step",
        1,
        &["Sonderzeichen/Übergröße + 100%.step"],
    )
    .await;
    assert_query(
        &router,
        &root_key,
        "directory=bulk&q=existiert-nicht&sort=path",
        0,
        &[],
    )
    .await;

    for sort in ["path", "format", "size", "modified"] {
        for direction in ["asc", "desc"] {
            assert_stable_pages(&router, &root_key, sort, direction).await;
        }
    }

    let (_, beyond) = get_json(
        &router,
        &format!("/api/v1/roots/{root_key}/folders?limit=2&offset=6"),
    )
    .await;
    assert_eq!(beyond["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        beyond["total"], 5,
        "empty pages must retain the filtered folder total"
    );
    fs::remove_dir_all(root).unwrap();
}

async fn assert_query(router: &axum::Router, root: &str, query: &str, total: i64, paths: &[&str]) {
    let (status, page) = get_json(router, &format!("/api/v1/roots/{root}/files?{query}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], total);
    let actual: Vec<_> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap())
        .collect();
    assert_eq!(actual, paths);
}

async fn assert_stable_pages(router: &axum::Router, root: &str, sort: &str, direction: &str) {
    let mut paths = Vec::new();
    for offset in [0, 37, 74, 111] {
        let (status, page) = get_json(router, &format!("/api/v1/roots/{root}/files?directory=bulk&q=teil-&sort={sort}&direction={direction}&limit=37&offset={offset}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 120);
        paths.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["path"].as_str().unwrap().to_owned()),
        );
    }
    assert_eq!(paths.len(), 120);
    let unique: std::collections::HashSet<_> = paths.iter().collect();
    assert_eq!(
        unique.len(),
        120,
        "{sort}/{direction} pages must neither overlap nor omit rows"
    );
    let (_, repeated) = get_json(router, &format!("/api/v1/roots/{root}/files?directory=bulk&q=teil-&sort={sort}&direction={direction}&limit=37&offset=37")).await;
    let repeated: Vec<_> = repeated["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        repeated,
        paths[37..74].iter().map(String::as_str).collect::<Vec<_>>()
    );
}
