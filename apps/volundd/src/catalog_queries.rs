use sqlx::{PgPool, Row};

use crate::api_models::{
    ArtifactSummary, ContentSummary, FileSummary, FolderSummary, Page, PreviewSummary, RootSummary,
    ScanSummary,
};
use crate::catalog_filter::CatalogFilter;

pub struct ArtifactRecord {
    pub relative_path: String,
    pub sha256: String,
    pub byte_size: i64,
    pub media_type: String,
}

pub async fn list_roots(pool: &PgPool) -> Result<Vec<RootSummary>, String> {
    let rows = sqlx::query(
        "SELECT root.public_id::text, root.root_key, root.display_name, root.read_only, \
         (SELECT count(*)::bigint FROM volund.source_files source \
          WHERE source.library_root_id = root.id AND source.lifecycle_state='available'), \
         (SELECT count(*)::bigint FROM volund.source_files source \
          WHERE source.library_root_id = root.id AND source.missing_at IS NOT NULL \
          AND source.lifecycle_state='available'), \
         (SELECT scan.status FROM volund.scan_runs scan \
          WHERE scan.library_root_id = root.id ORDER BY scan.id DESC LIMIT 1) \
         FROM volund.library_roots root ORDER BY root.root_key",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list library roots: {error}"))?;
    Ok(rows
        .into_iter()
        .map(|row| RootSummary {
            id: row.get(0),
            key: row.get(1),
            name: row.get(2),
            read_only: row.get(3),
            file_count: row.get(4),
            missing_file_count: row.get(5),
            latest_scan_status: row.get(6),
        })
        .collect())
}

pub async fn list_files(
    pool: &PgPool,
    root_key: &str,
    filter: &CatalogFilter,
) -> Result<Option<Page<FileSummary>>, String> {
    let Some(root_id) = root_id(pool, root_key).await? else {
        return Ok(None);
    };
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint FROM volund.source_files \
         JOIN volund.content_objects content ON content.id = source_files.content_object_id \
         WHERE library_root_id = $1 AND lifecycle_state='available' AND ($2 OR missing_at IS NULL) \
         AND ($3::text IS NULL OR content.detected_format = $3) \
         AND ($4 = '' OR left(relative_path, char_length($4) + 1) = $4 || '/') \
         AND ($5 = '' OR strpos(lower(relative_path), lower($5)) > 0) \
         AND ($5 <> '' OR strpos(CASE WHEN $4 = '' THEN relative_path \
              ELSE substring(relative_path FROM char_length($4) + 2) END, '/') = 0)",
    )
    .bind(root_id)
    .bind(filter.include_missing)
    .bind(&filter.format)
    .bind(&filter.directory)
    .bind(&filter.query)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count source files: {error}"))?;
    let rows = sqlx::query(
        "SELECT source.public_id::text, source.relative_path, content.sha256, \
         content.byte_size, content.detected_format, \
         round(extract(epoch FROM source.filesystem_modified_at) * 1000)::bigint, \
         source.missing_at IS NOT NULL \
         FROM volund.source_files source \
         JOIN volund.content_objects content ON content.id = source.content_object_id \
         WHERE source.library_root_id = $1 AND source.lifecycle_state='available' \
         AND ($2 OR source.missing_at IS NULL) \
         AND ($3::text IS NULL OR content.detected_format = $3) \
         AND ($4 = '' OR left(source.relative_path, char_length($4) + 1) = $4 || '/') \
         AND ($5 = '' OR strpos(lower(source.relative_path), lower($5)) > 0) \
         AND ($5 <> '' OR strpos(CASE WHEN $4 = '' THEN source.relative_path \
              ELSE substring(source.relative_path FROM char_length($4) + 2) END, '/') = 0) \
         ORDER BY \
         CASE WHEN $6 = 'path' AND $7 = 'asc' THEN lower(source.relative_path) END ASC NULLS LAST, \
         CASE WHEN $6 = 'path' AND $7 = 'desc' THEN lower(source.relative_path) END DESC NULLS LAST, \
         CASE WHEN $6 = 'format' AND $7 = 'asc' THEN content.detected_format END ASC NULLS LAST, \
         CASE WHEN $6 = 'format' AND $7 = 'desc' THEN content.detected_format END DESC NULLS LAST, \
         CASE WHEN $6 = 'size' AND $7 = 'asc' THEN content.byte_size END ASC NULLS LAST, \
         CASE WHEN $6 = 'size' AND $7 = 'desc' THEN content.byte_size END DESC NULLS LAST, \
         CASE WHEN $6 = 'modified' AND $7 = 'asc' THEN source.filesystem_modified_at END ASC NULLS LAST, \
         CASE WHEN $6 = 'modified' AND $7 = 'desc' THEN source.filesystem_modified_at END DESC NULLS LAST, \
         source.relative_path ASC LIMIT $8 OFFSET $9",
    )
    .bind(root_id)
    .bind(filter.include_missing)
    .bind(&filter.format)
    .bind(&filter.directory)
    .bind(&filter.query)
    .bind(&filter.sort)
    .bind(&filter.direction)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list source files: {error}"))?;
    Ok(Some(Page {
        items: rows
            .into_iter()
            .map(|row| FileSummary {
                id: row.get(0),
                path: row.get(1),
                sha256: row.get(2),
                byte_size: row.get(3),
                format: row.get(4),
                modified_at_unix_ms: row.get(5),
                missing: row.get(6),
            })
            .collect(),
        limit: filter.limit,
        offset: filter.offset,
        total,
    }))
}

pub async fn list_folders(
    pool: &PgPool,
    root_key: &str,
    filter: &CatalogFilter,
) -> Result<Option<Page<FolderSummary>>, String> {
    let Some(root_id) = root_id(pool, root_key).await? else {
        return Ok(None);
    };
    let total = sqlx::query_scalar::<_, i64>(
        "WITH filtered AS ( \
           SELECT CASE WHEN $4 = '' THEN source.relative_path \
                  ELSE substring(source.relative_path FROM char_length($4) + 2) END AS remainder \
           FROM volund.source_files source \
           JOIN volund.content_objects content ON content.id = source.content_object_id \
           WHERE source.library_root_id = $1 AND source.lifecycle_state='available' \
           AND ($2 OR source.missing_at IS NULL) \
           AND ($3::text IS NULL OR content.detected_format = $3) \
           AND ($4 = '' OR left(source.relative_path, char_length($4) + 1) = $4 || '/') \
           AND ($5 = '' OR strpos(lower(source.relative_path), lower($5)) > 0) \
         ), folders AS (SELECT split_part(remainder, '/', 1) AS name \
           FROM filtered WHERE strpos(remainder, '/') > 0 GROUP BY name) \
         SELECT count(*)::bigint FROM folders",
    )
    .bind(root_id)
    .bind(filter.include_missing)
    .bind(&filter.format)
    .bind(&filter.directory)
    .bind(&filter.query)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count catalog folders: {error}"))?;
    let rows = sqlx::query(
        "WITH filtered AS ( \
           SELECT CASE WHEN $4 = '' THEN source.relative_path \
                  ELSE substring(source.relative_path FROM char_length($4) + 2) END AS remainder \
           FROM volund.source_files source \
           JOIN volund.content_objects content ON content.id = source.content_object_id \
           WHERE source.library_root_id = $1 AND source.lifecycle_state='available' \
           AND ($2 OR source.missing_at IS NULL) \
           AND ($3::text IS NULL OR content.detected_format = $3) \
           AND ($4 = '' OR left(source.relative_path, char_length($4) + 1) = $4 || '/') \
           AND ($5 = '' OR strpos(lower(source.relative_path), lower($5)) > 0) \
         ), folders AS ( \
           SELECT split_part(remainder, '/', 1) AS name, count(*)::bigint AS file_count \
           FROM filtered WHERE strpos(remainder, '/') > 0 GROUP BY name \
         ) \
         SELECT name, CASE WHEN $4 = '' THEN name ELSE $4 || '/' || name END, file_count \
         FROM folders ORDER BY lower(name) LIMIT $6 OFFSET $7",
    )
    .bind(root_id)
    .bind(filter.include_missing)
    .bind(&filter.format)
    .bind(&filter.directory)
    .bind(&filter.query)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list catalog folders: {error}"))?;
    Ok(Some(Page {
        items: rows
            .into_iter()
            .map(|row| FolderSummary {
                name: row.get(0),
                path: row.get(1),
                file_count: row.get(2),
            })
            .collect(),
        limit: filter.limit,
        offset: filter.offset,
        total,
    }))
}

pub async fn list_scans(
    pool: &PgPool,
    root_key: &str,
    limit: i64,
    offset: i64,
) -> Result<Option<Page<ScanSummary>>, String> {
    let Some(root_id) = root_id(pool, root_key).await? else {
        return Ok(None);
    };
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint FROM volund.scan_runs WHERE library_root_id = $1",
    )
    .bind(root_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count scan runs: {error}"))?;
    let rows = sqlx::query(
        "SELECT public_id::text, status, \
         round(extract(epoch FROM started_at) * 1000)::bigint, \
         round(extract(epoch FROM finished_at) * 1000)::bigint, \
         discovered_files, hashed_files, missing_files \
         FROM volund.scan_runs WHERE library_root_id = $1 \
         ORDER BY id DESC LIMIT $2 OFFSET $3",
    )
    .bind(root_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list scan runs: {error}"))?;
    Ok(Some(Page {
        items: rows
            .into_iter()
            .map(|row| ScanSummary {
                id: row.get(0),
                status: row.get(1),
                started_at_unix_ms: row.get(2),
                finished_at_unix_ms: row.get(3),
                discovered_files: row.get(4),
                hashed_files: row.get(5),
                missing_files: row.get(6),
            })
            .collect(),
        limit,
        offset,
        total,
    }))
}

pub async fn get_content(pool: &PgPool, sha256: &str) -> Result<Option<ContentSummary>, String> {
    let row = sqlx::query(
        "SELECT content.sha256, content.byte_size, content.detected_format, \
         count(source.id)::bigint, \
         count(source.id) FILTER (WHERE source.missing_at IS NULL)::bigint \
         FROM volund.content_objects content \
         LEFT JOIN volund.source_files source ON source.content_object_id = content.id \
         WHERE content.sha256 = $1 \
         GROUP BY content.id, content.sha256, content.byte_size, content.detected_format",
    )
    .bind(sha256)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot load content object: {error}"))?;
    Ok(row.map(|row| ContentSummary {
        sha256: row.get(0),
        byte_size: row.get(1),
        format: row.get(2),
        source_count: row.get(3),
        available_source_count: row.get(4),
    }))
}

pub async fn list_previews(
    pool: &PgPool,
    file_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Option<Page<PreviewSummary>>, String> {
    let content_id = sqlx::query_scalar::<_, i64>(
        "SELECT content_object_id FROM volund.source_files WHERE public_id::text = $1",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot find preview source: {error}"))?;
    let Some(content_id) = content_id else {
        return Ok(None);
    };
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint FROM volund.conversion_runs WHERE content_object_id = $1",
    )
    .bind(content_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("cannot count previews: {error}"))?;
    let rows = sqlx::query(
        "SELECT public_id::text, profile, status, converter_version, \
         round(extract(epoch FROM requested_at) * 1000)::bigint, \
         round(extract(epoch FROM finished_at) * 1000)::bigint \
         FROM volund.conversion_runs WHERE content_object_id = $1 \
         ORDER BY id DESC LIMIT $2 OFFSET $3",
    )
    .bind(content_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("cannot list previews: {error}"))?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.get(0);
        let artifact_rows = sqlx::query(
            "SELECT artifact.artifact_kind, artifact.sha256, artifact.byte_size, artifact.media_type \
             FROM volund.derived_artifacts artifact \
             JOIN volund.conversion_runs run ON run.id = artifact.conversion_run_id \
             WHERE run.public_id::text = $1 ORDER BY artifact.artifact_kind",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("cannot list preview artifacts: {error}"))?;
        items.push(PreviewSummary {
            id: id.clone(),
            profile: row.get(1),
            status: row.get(2),
            converter_version: row.get(3),
            requested_at_unix_ms: row.get(4),
            finished_at_unix_ms: row.get(5),
            artifacts: artifact_rows
                .into_iter()
                .map(|artifact| {
                    let kind: String = artifact.get(0);
                    ArtifactSummary {
                        url: format!("/api/v1/previews/{id}/artifacts/{kind}"),
                        kind,
                        sha256: artifact.get(1),
                        byte_size: artifact.get(2),
                        media_type: artifact.get(3),
                    }
                })
                .collect(),
        });
    }
    Ok(Some(Page {
        items,
        limit,
        offset,
        total,
    }))
}

pub async fn get_artifact(
    pool: &PgPool,
    preview_id: &str,
    kind: &str,
) -> Result<Option<ArtifactRecord>, String> {
    let row = sqlx::query(
        "SELECT artifact.relative_path, artifact.sha256, artifact.byte_size, artifact.media_type \
         FROM volund.derived_artifacts artifact \
         JOIN volund.conversion_runs run ON run.id = artifact.conversion_run_id \
         WHERE run.public_id::text = $1 AND run.status = 'ready' \
         AND artifact.artifact_kind = $2",
    )
    .bind(preview_id)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot find preview artifact: {error}"))?;
    Ok(row.map(|row| ArtifactRecord {
        relative_path: row.get(0),
        sha256: row.get(1),
        byte_size: row.get(2),
        media_type: row.get(3),
    }))
}

pub async fn get_artifact_by_id(
    pool: &PgPool,
    artifact_id: &str,
) -> Result<Option<ArtifactRecord>, String> {
    let row = sqlx::query(
        "SELECT artifact.relative_path,artifact.sha256,artifact.byte_size,artifact.media_type \
         FROM volund.derived_artifacts artifact JOIN volund.conversion_runs run \
         ON run.id=artifact.conversion_run_id WHERE artifact.public_id::text=$1 AND run.status='ready'",
    )
    .bind(artifact_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot find derived artifact: {error}"))?;
    Ok(row.map(|row| ArtifactRecord {
        relative_path: row.get(0),
        sha256: row.get(1),
        byte_size: row.get(2),
        media_type: row.get(3),
    }))
}

async fn root_id(pool: &PgPool, root_key: &str) -> Result<Option<i64>, String> {
    sqlx::query_scalar("SELECT id FROM volund.library_roots WHERE root_key = $1")
        .bind(root_key)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("cannot find library root: {error}"))
}
