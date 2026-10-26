#![cfg(target_os = "linux")]
//! Server-side model search (ADR 0032): filters, sorting, pagination, totals.
mod support;

use serde_json::Value;
use support::{authenticated_router, get_json, test_database};
use volundd::scanner;

fn names(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One fixture catalog covers every search dimension.
async fn model_search_filters_sorts_and_pages_on_the_server() {
    let Some(pool) = test_database().await else {
        return;
    };
    let library = std::env::temp_dir().join(format!("volund-search-{}", std::process::id()));
    std::fs::create_dir_all(&library).unwrap();
    for name in ["a.step", "b.step", "c.step"] {
        std::fs::write(library.join(name), name.as_bytes()).unwrap();
    }
    scanner::register_root(&pool, "search", "Search", &library)
        .await
        .unwrap();
    scanner::scan_root(&pool, "search", false).await.unwrap();
    // Four models with distinct kinds, update times, authors, and tags.
    sqlx::query(
        "WITH author AS (INSERT INTO volund.authors (name,normalized_name) VALUES ('Ada Maker','ada maker') RETURNING id), \
         tag AS (INSERT INTO volund.tags (slug,name,normalized_name) VALUES ('halterung','Halterung','halterung') RETURNING id) \
         INSERT INTO volund.models (slug,name,kind,description,author_id,created_at,updated_at) VALUES \
         ('bracket','Bracket','part','Wall mount',(SELECT id FROM author),now()-interval '4 days',now()-interval '1 day'), \
         ('gearbox','Gearbox','assembly','Two-stage',NULL,now()-interval '3 days',now()-interval '3 days'), \
         ('alpha-rig','Alpha Rig','project','50% scale',NULL,now()-interval '2 days',now()-interval '2 days'), \
         ('zeta','Zeta','part','',NULL,now()-interval '1 day',now())",
    )
    .execute(&*pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO volund.model_tags (model_id,tag_id) SELECT model.id,tag.id \
         FROM volund.models model, volund.tags tag WHERE model.slug IN ('bracket','zeta')",
    )
    .execute(&*pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO volund.model_source_files (model_id,source_file_id,role,is_primary,ordinal) \
         SELECT model.id,source.id,'master-cad',source.relative_path='a.step',row_number() OVER () \
         FROM volund.models model, volund.source_files source WHERE model.slug='gearbox'",
    )
    .execute(&*pool)
    .await
    .unwrap();
    let router = authenticated_router(&pool, volundd::api::router((*pool).clone())).await;
    let search = |query: &str| {
        let router = router.clone();
        let uri = format!("/api/v1/models/search{query}");
        async move { get_json(&router, &uri).await }
    };

    let (status, all) = search("").await;
    assert_eq!(status, 200);
    assert_eq!(names(&all), ["Zeta", "Bracket", "Alpha Rig", "Gearbox"]);
    assert_eq!(
        (all["total"].as_i64(), all["fileLinkTotal"].as_i64()),
        (Some(4), Some(3))
    );

    let (_, by_name) = search("?sort=name&limit=2&offset=1").await;
    assert_eq!(names(&by_name), ["Bracket", "Gearbox"]);
    assert_eq!(
        (
            by_name["total"].as_i64(),
            by_name["limit"].as_i64(),
            by_name["offset"].as_i64()
        ),
        (Some(4), Some(2), Some(1))
    );
    let (_, created) = search("?sort=created").await;
    assert_eq!(names(&created), ["Zeta", "Alpha Rig", "Gearbox", "Bracket"]);

    // Text matches name, description, author, and tag names, case-insensitively.
    for (query, expected) in [
        ("?q=GEAR", vec!["Gearbox"]),
        ("?q=wall", vec!["Bracket"]),
        ("?q=ada", vec!["Bracket"]),
        ("?q=halter", vec!["Zeta", "Bracket"]),
        ("?q=50%25", vec!["Alpha Rig"]),
        ("?q=%25", vec!["Alpha Rig"]),
        ("?q=_", vec![]),
    ] {
        assert_eq!(names(&search(query).await.1), expected, "{query}");
    }
    let tag: String = sqlx::query_scalar("SELECT public_id::text FROM volund.tags")
        .fetch_one(&*pool)
        .await
        .unwrap();
    let (_, tagged) = search(&format!("?tag={tag}&kind=part&sort=name")).await;
    assert_eq!(names(&tagged), ["Bracket", "Zeta"]);
    assert_eq!(tagged["fileLinkTotal"].as_i64(), Some(0));

    sqlx::query("UPDATE volund.models SET active=false, removed_at=now() WHERE slug='zeta'")
        .execute(&*pool)
        .await
        .unwrap();
    assert_eq!(
        search("").await.1["total"].as_i64(),
        Some(3),
        "removed models are excluded"
    );

    for invalid in [
        "?sort=random",
        "?kind=statue",
        "?tag=nope",
        "?limit=0",
        "?limit=201",
        "?offset=-1",
    ] {
        let (status, body) = search(invalid).await;
        assert_eq!(status, 400, "{invalid}: {body}");
    }
    std::fs::remove_dir_all(library).unwrap();
}
