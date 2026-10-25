//! Versioned, machine-readable instance metadata export (ADR 0029).
//!
//! One consistent snapshot of every user-authored catalog fact, keyed only by
//! public identifiers. Secrets (credentials, sessions, invitation and slicer
//! tokens), host paths, derived artifacts and transient import or job state are
//! omitted; the document lists them under `omitted`.

use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::catalog_history;
use crate::database::EXPECTED_MIGRATIONS;

pub const FORMAT: &str = "volund-metadata-v1";
pub const FORMAT_VERSION: u32 = 1;

const MS: &str = "round(extract(epoch FROM {})*1000)::bigint";

/// Sections built entirely in `PostgreSQL`; each query returns one JSON array.
const SECTIONS: &[(&str, &str)] = &[
    (
        "libraryRoots",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',public_id,'key',root_key,\
         'displayName',display_name,'enabled',enabled) ORDER BY root_key),'[]') \
         FROM volund.library_roots",
    ),
    (
        "sourceFiles",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',source.public_id,'rootKey',root.root_key,\
         'relativePath',source.relative_path,'sha256',content.sha256,'byteSize',content.byte_size,\
         'detectedFormat',content.detected_format,'mediaType',content.media_type,\
         'lifecycleState',source.lifecycle_state,'missing',source.missing_at IS NOT NULL,\
         'firstSeenAtUnixMs',{first_seen}) ORDER BY root.root_key,source.relative_path),'[]') \
         FROM volund.source_files source \
         JOIN volund.library_roots root ON root.id=source.library_root_id \
         JOIN volund.content_objects content ON content.id=source.content_object_id",
    ),
    (
        "fileDependencies",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('sourceFileId',source.public_id,\
         'kind',dependency.dependency_kind,'reference',dependency.raw_reference,\
         'resolvedSourceFileId',resolved.public_id) ORDER BY dependency.id),'[]') \
         FROM volund.file_dependencies dependency \
         JOIN volund.source_files source ON source.id=dependency.source_file_id \
         LEFT JOIN volund.source_files resolved ON resolved.id=dependency.resolved_source_file_id",
    ),
    (
        "fileMoves",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',move.public_id,'sourceFileId',source.public_id,\
         'previousRelativePath',move.previous_relative_path,'newRelativePath',move.new_relative_path,\
         'movedAtUnixMs',{moved_at}) ORDER BY move.moved_at,move.id),'[]') \
         FROM volund.source_file_moves move JOIN volund.source_files source ON source.id=move.source_file_id",
    ),
    (
        "authors",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',author.public_id,'name',author.name,\
         'website',author.website,'provenanceSource',author.provenance_source,\
         'provenanceNote',author.provenance_note,'active',author.active,\
         'mergedIntoId',target.public_id,'revision',author.revision) ORDER BY author.name,author.id),'[]') \
         FROM volund.authors author LEFT JOIN volund.authors target ON target.id=author.merged_into_id",
    ),
    (
        "tags",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',tag.public_id,'slug',tag.slug,'name',tag.name,\
         'active',tag.active,'mergedIntoId',target.public_id,'revision',tag.revision) \
         ORDER BY tag.name,tag.id),'[]') \
         FROM volund.tags tag LEFT JOIN volund.tags target ON target.id=tag.merged_into_id",
    ),
    (
        "collections",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',collection.public_id,'slug',collection.slug,\
         'name',collection.name,'description',collection.description,'active',collection.active,\
         'revision',collection.revision,'modelIds',coalesce((SELECT jsonb_agg(model.public_id \
         ORDER BY member.ordinal,model.id) FROM volund.collection_models member \
         JOIN volund.models model ON model.id=member.model_id \
         WHERE member.collection_id=collection.id),'[]')) ORDER BY collection.name,collection.id),'[]') \
         FROM volund.collections collection",
    ),
    (
        "models",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',model.public_id,'slug',model.slug,\
         'name',model.name,'description',model.description,'kind',model.kind,\
         'authorId',author.public_id,'licenseKind',model.license_kind,'licenseValue',model.license_value,\
         'viewerRotation',jsonb_build_array(model.viewer_rotation_x,model.viewer_rotation_y,model.viewer_rotation_z),\
         'active',model.active,'revision',model.revision,\
         'createdAtUnixMs',{model_created},'updatedAtUnixMs',{model_updated},\
         'thumbnail',jsonb_build_object('kind',model.thumbnail_kind,'sourceFileId',thumbnail.public_id),\
         'tagIds',coalesce((SELECT jsonb_agg(tag.public_id ORDER BY tag.name) FROM volund.model_tags link \
           JOIN volund.tags tag ON tag.id=link.tag_id WHERE link.model_id=model.id),'[]'),\
         'componentIds',coalesce((SELECT jsonb_agg(child.public_id ORDER BY component.ordinal) \
           FROM volund.model_components component JOIN volund.models child ON child.id=component.child_model_id \
           WHERE component.parent_model_id=model.id),'[]'),\
         'files',coalesce((SELECT jsonb_agg(jsonb_build_object('sourceFileId',source.public_id,\
           'role',link.role,'primary',link.is_primary,'ordinal',link.ordinal,'revision',link.revision,\
           'caption',link.caption,'description',link.description,'notes',link.notes,\
           'printable',link.printable,'printed',link.printed,'preSupported',link.pre_supported,\
           'upAxis',link.up_axis,'supportHint',link.support_hint,\
           'orientation',jsonb_build_array(link.orientation_x,link.orientation_y,link.orientation_z)) \
           ORDER BY link.ordinal,source.id) FROM volund.model_source_files link \
           JOIN volund.source_files source ON source.id=link.source_file_id WHERE link.model_id=model.id),'[]')\
         ) ORDER BY model.slug,model.id),'[]') \
         FROM volund.models model \
         LEFT JOIN volund.authors author ON author.id=model.author_id \
         LEFT JOIN volund.source_files thumbnail ON thumbnail.id=model.thumbnail_source_file_id",
    ),
    (
        "users",
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id',public_id,'email',email,\
         'displayName',display_name,'role',role,'status',status,'createdAtUnixMs',{user_created}) \
         ORDER BY created_at,id),'[]') FROM volund.users",
    ),
];

/// Build the export document from one read-only, repeatable-read snapshot.
///
/// # Errors
///
/// Returns a safe message when the snapshot cannot be read.
pub async fn build(pool: &PgPool, created_at_unix_ms: i64) -> Result<Value, String> {
    let failed = |error: sqlx::Error| format!("cannot export metadata: {error}");
    let mut transaction = pool.begin().await.map_err(failed)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(failed)?;
    let mut document = Map::new();
    let mut counts = Map::new();
    for (name, template) in SECTIONS {
        let value: Value = sqlx::query_scalar(&section_sql(template))
            .fetch_one(&mut *transaction)
            .await
            .map_err(failed)?;
        counts.insert(
            (*name).to_owned(),
            json!(value.as_array().map_or(0, Vec::len)),
        );
        document.insert((*name).to_owned(), value);
    }
    let history = catalog_history_events(&mut transaction).await?;
    counts.insert("catalogHistory".to_owned(), json!(history.len()));
    document.insert("catalogHistory".to_owned(), Value::Array(history));
    transaction.commit().await.map_err(failed)?;
    let mut root = Map::new();
    root.insert("format".to_owned(), json!(FORMAT));
    root.insert("formatVersion".to_owned(), json!(FORMAT_VERSION));
    root.insert("volundVersion".to_owned(), json!(env!("CARGO_PKG_VERSION")));
    root.insert("schemaMigrations".to_owned(), json!(EXPECTED_MIGRATIONS));
    root.insert("createdAtUnixMs".to_owned(), json!(created_at_unix_ms));
    root.insert("counts".to_owned(), Value::Object(counts));
    root.extend(document);
    root.insert(
        "omitted".to_owned(),
        json!([
            "credentials",
            "sessions",
            "invitations",
            "slicer-handoff-tokens",
            "library-host-paths",
            "derived-artifacts",
            "conversion-runs",
            "import-drafts",
            "jobs-and-logs",
            "settings-and-preferences"
        ]),
    );
    Ok(Value::Object(root))
}

/// Expand the `{name}` placeholders into millisecond epoch expressions.
fn section_sql(template: &str) -> String {
    [
        ("{first_seen}", "source.first_seen_at"),
        ("{moved_at}", "move.moved_at"),
        ("{model_created}", "model.created_at"),
        ("{model_updated}", "model.updated_at"),
        ("{user_created}", "created_at"),
    ]
    .iter()
    .fold(template.to_owned(), |sql, (placeholder, column)| {
        sql.replace(placeholder, &MS.replace("{}", column))
    })
}

async fn catalog_history_events(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<Value>, String> {
    let target_types: Vec<String> = catalog_history::TARGET_TYPES
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    let rows = sqlx::query(
        "SELECT public_id::text,actor_public_id::text,actor_display_name, \
         round(extract(epoch FROM occurred_at)*1000)::bigint,action,outcome,target_type, \
         target_public_id::text,metadata FROM volund.security_audit_events \
         WHERE target_type=ANY($1) ORDER BY occurred_at,id",
    )
    .bind(&target_types)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| format!("cannot export catalog history: {error}"))?;
    Ok(rows
        .iter()
        .map(|row| {
            let metadata: Value = row.get(8);
            json!({
                "id": row.get::<String, _>(0),
                "actorId": row.get::<Option<String>, _>(1),
                "actorName": row.get::<Option<String>, _>(2),
                "occurredAtUnixMs": row.get::<i64, _>(3),
                "action": row.get::<String, _>(4),
                "outcome": row.get::<String, _>(5),
                "targetType": row.get::<String, _>(6),
                "targetId": row.get::<Option<String>, _>(7),
                "change": catalog_history::sanitized(&metadata),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{SECTIONS, section_sql};

    #[test]
    fn every_timestamp_placeholder_is_expanded() {
        for (name, template) in SECTIONS {
            let sql = section_sql(template);
            assert!(
                !sql.contains("{first_seen}") && !sql.contains("{moved_at}"),
                "{name}"
            );
            assert!(
                !sql.contains("{model_") && !sql.contains("{user_created}"),
                "{name}"
            );
        }
        assert!(section_sql("x {moved_at} y").contains("extract(epoch FROM move.moved_at)"));
    }

    #[test]
    fn sections_never_select_secrets_or_host_paths() {
        for (name, template) in SECTIONS {
            for forbidden in [
                "filesystem_path",
                "password",
                "token",
                "digest",
                "client_address",
            ] {
                assert!(
                    !template.contains(forbidden),
                    "{name} must not read {forbidden}"
                );
            }
        }
    }
}
