use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use unicode_normalization::UnicodeNormalization;
use volund_core::normalize_library_path;

const MAX_FILES: usize = 10_000;
const MAX_ITEM_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const MAX_RATIO: u64 = 100;
const MAX_PATH_CHARS: usize = 1024;
const MAX_DEPTH: usize = 32;
const DEADLINE: Duration = Duration::from_secs(30 * 60);

#[derive(Debug)]
struct Expanded {
    original_path: String,
    byte_size: i64,
    sha256: String,
    category: &'static str,
    part_path: PathBuf,
}

#[derive(Debug)]
struct ExtractionParts {
    directory: PathBuf,
    attempt_key: String,
}

impl Drop for ExtractionParts {
    fn drop(&mut self) {
        cleanup_parts(&self.directory, &self.attempt_key);
    }
}

// The blocking worker owns cleanup until it hands both result and guard back.
// Dropping its async waiter cannot clean too early while the worker still writes.
async fn extract_in_background(
    parts: ExtractionParts,
    extract: impl FnOnce(&ExtractionParts) -> Result<Vec<Expanded>, String> + Send + 'static,
) -> Result<(Vec<Expanded>, ExtractionParts), String> {
    tokio::task::spawn_blocking(move || {
        let expanded = extract(&parts)?;
        Ok((expanded, parts))
    })
    .await
    .map_err(|error| format!("cannot join ZIP inspection: {error}"))?
}

/// Inspect and expand one ZIP container into verified ordinary staged items.
///
/// # Errors
/// Refuses unsafe or excessive archives and leaves the original staged ZIP retryable.
pub async fn expand_zip(
    pool: &PgPool,
    incoming_root: &Path,
    draft_id: &str,
    item_id: &str,
) -> Result<(), String> {
    let row = sqlx::query(
        "SELECT i.archive_expanded_at IS NOT NULL,d.suggested_slug,gen_random_uuid()::text \
         FROM volund.import_draft_items i JOIN volund.import_drafts d ON d.id=i.import_draft_id \
         WHERE d.public_id::text=$1 AND i.public_id::text=$2 AND i.category='archive'",
    )
    .bind(draft_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("cannot inspect archive item: {error}"))?
    .ok_or_else(|| "unknown ZIP import item".to_owned())?;
    if row.get::<bool, _>(0) {
        return Ok(());
    }
    let slug: String = row.get(1);
    let directory = incoming_root.join(draft_id);
    let archive_path = directory.join(format!("{item_id}.bin"));
    // Each attempt owns only its own temporary files, including error cleanup.
    let attempt_key = format!("{item_id}-{}", row.get::<String, _>(2));
    let (expanded, _parts) = extract_in_background(
        ExtractionParts {
            directory,
            attempt_key,
        },
        move |parts| inspect_and_extract(&archive_path, &parts.directory, &parts.attempt_key),
    )
    .await?;
    publish_expanded(pool, draft_id, item_id, &slug, expanded).await
}

fn inspect_and_extract(
    archive_path: &Path,
    directory: &Path,
    item_id: &str,
) -> Result<Vec<Expanded>, String> {
    let result = inspect_and_extract_inner(archive_path, directory, item_id);
    if result.is_err() {
        cleanup_parts(directory, item_id);
    }
    result
}

fn inspect_and_extract_inner(
    archive_path: &Path,
    directory: &Path,
    item_id: &str,
) -> Result<Vec<Expanded>, String> {
    let mut signature = [0_u8; 4];
    File::open(archive_path)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|_| "zip_header_invalid".to_owned())?;
    if !matches!(
        signature,
        [b'P', b'K', 3, 4] | [b'P', b'K', 5, 6] | [b'P', b'K', 7, 8]
    ) {
        return Err("zip_signature_invalid".to_owned());
    }
    let file = File::open(archive_path).map_err(|_| "zip_open_failed".to_owned())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "zip_directory_invalid".to_owned())?;
    if archive.len() > MAX_FILES {
        return Err("zip_item_limit".to_owned());
    }
    let started = Instant::now();
    let mut seen = HashSet::with_capacity(archive.len());
    let mut total = 0_u64;
    let mut output = Vec::new();
    for index in 0..archive.len() {
        if started.elapsed() > DEADLINE {
            return Err("zip_time_limit".to_owned());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "zip_entry_invalid".to_owned())?;
        if entry.is_dir() {
            continue;
        }
        if entry.encrypted() {
            return Err("zip_encrypted_refused".to_owned());
        }
        ensure_regular(&entry)?;
        let path = normalize_archive_path(entry.name())?;
        if is_archive_path(&path) {
            return Err("zip_nested_archive_refused".to_owned());
        }
        let collision = path.nfkc().collect::<String>().to_lowercase();
        if !seen.insert(collision) {
            return Err("zip_path_collision".to_owned());
        }
        let size = entry.size();
        if size > MAX_ITEM_BYTES {
            return Err("zip_item_size_limit".to_owned());
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| "zip_total_size_limit".to_owned())?;
        if total > MAX_TOTAL_BYTES {
            return Err("zip_total_size_limit".to_owned());
        }
        let compressed = entry.compressed_size();
        if size > compressed.max(1).saturating_mul(MAX_RATIO) {
            return Err("zip_ratio_limit".to_owned());
        }
        let part_path = directory.join(format!(".{item_id}-{index}.extract.part"));
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
            .map_err(|_| "zip_extract_target_failed".to_owned())?;
        let mut digest = Sha256::new();
        let copied = copy_bounded(&mut entry, &mut target, &mut digest)?;
        if copied != size {
            fs::remove_file(&part_path).ok();
            return Err("zip_size_mismatch".to_owned());
        }
        target
            .sync_all()
            .map_err(|_| "zip_extract_sync_failed".to_owned())?;
        output.push(Expanded {
            original_path: path.clone(),
            byte_size: i64::try_from(copied).map_err(|_| "zip_item_size_limit".to_owned())?,
            sha256: format!("{:x}", digest.finalize()),
            category: classify(&path),
            part_path,
        });
    }
    if output.is_empty() {
        return Err("zip_empty_refused".to_owned());
    }
    Ok(output)
}

fn copy_bounded(
    input: &mut impl Read,
    output: &mut impl Write,
    digest: &mut Sha256,
) -> Result<u64, String> {
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|_| "zip_decompression_failed".to_owned())?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > MAX_ITEM_BYTES {
            return Err("zip_item_size_limit".to_owned());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| "zip_extract_write_failed".to_owned())?;
        digest.update(&buffer[..read]);
    }
    Ok(total)
}

async fn publish_expanded(
    pool: &PgPool,
    draft_id: &str,
    item_id: &str,
    slug: &str,
    expanded: Vec<Expanded>,
) -> Result<(), String> {
    let policy = crate::import_policy::load(pool)
        .await
        .map_err(|_| "zip_capacity_policy_invalid".to_owned())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("cannot begin ZIP publication: {error}"))?;
    let reserved = crate::import_capacity::lock_and_used(&mut tx)
        .await
        .map_err(|_| "zip_capacity_check_failed".to_owned())?;
    let draft_internal = lock_active_draft(&mut tx, draft_id).await?;
    if already_published(&mut tx, draft_internal, item_id).await? {
        return Ok(());
    }
    let added_bytes = expanded
        .iter()
        .try_fold(0_i64, |total, value| total.checked_add(value.byte_size))
        .ok_or_else(|| "zip_total_size_limit".to_owned())?;
    if reserved.saturating_add(added_bytes) > policy.incoming_capacity_bytes {
        return Err("zip_incoming_capacity_exceeded".to_owned());
    }
    let mut publications = Vec::new();
    for value in &expanded {
        let suggested = format!(
            "{slug}/{}/{}",
            category_directory(value.category),
            value.original_path
        );
        let public_id: String = sqlx::query_scalar(
            "INSERT INTO volund.import_draft_items(import_draft_id,original_path,byte_size,category, \
             suggested_relative_path,is_primary_candidate,upload_status,uploaded_bytes,sha256, \
             upload_started_at,upload_completed_at) VALUES($1,$2,$3,$4,$5,false,'uploaded',$3,$6,now(),now()) \
             RETURNING public_id::text",
        ).bind(draft_internal).bind(&value.original_path).bind(value.byte_size).bind(value.category)
            .bind(suggested).bind(&value.sha256).fetch_one(&mut *tx).await
            .map_err(|error| format!("cannot persist ZIP entry: {error}"))?;
        let final_path = value.part_path.with_file_name(format!("{public_id}.bin"));
        publications.push((value.part_path.clone(), final_path));
    }
    sqlx::query(
        "UPDATE volund.import_draft_items SET archive_expanded_at=now(),planned_action='skip',resolution='skip' \
         WHERE import_draft_id=$1 AND public_id::text=$2",
    ).bind(draft_internal).bind(item_id).execute(&mut *tx).await
        .map_err(|error| format!("cannot complete ZIP item: {error}"))?;
    sqlx::query(
        "UPDATE volund.import_drafts SET total_files=total_files+$2,total_bytes=total_bytes+$3, \
         uploaded_bytes=uploaded_bytes+$3, \
         status=CASE WHEN EXISTS(SELECT 1 FROM volund.import_draft_items \
         WHERE import_draft_id=$1 AND upload_status<>'uploaded') THEN 'uploading' ELSE 'uploaded' END, \
         updated_at=now(), \
         primary_item_id=COALESCE(primary_item_id,(SELECT id FROM volund.import_draft_items \
         WHERE import_draft_id=$1 AND category='cad' ORDER BY byte_size DESC,id LIMIT 1)) WHERE id=$1",
    )
    .bind(draft_internal)
    .bind(i32::try_from(expanded.len()).map_err(|_| "zip_item_limit".to_owned())?)
    .bind(added_bytes)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("cannot update ZIP draft: {error}"))?;
    sqlx::query(
        "UPDATE volund.import_draft_items SET is_primary_candidate=true WHERE id=( \
         SELECT primary_item_id FROM volund.import_drafts WHERE id=$1)",
    )
    .bind(draft_internal)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("cannot mark ZIP primary candidate: {error}"))?;
    let mut published = Vec::new();
    for (part_path, final_path) in &publications {
        if let Err(error) = fs::rename(part_path, final_path) {
            for path in &published {
                fs::remove_file(path).ok();
            }
            return Err(format!("cannot publish ZIP entry: {error}"));
        }
        published.push(final_path.clone());
    }
    if let Err(error) = tx.commit().await {
        for path in &published {
            fs::remove_file(path).ok();
        }
        return Err(format!("cannot commit ZIP publication: {error}"));
    }
    Ok(())
}

async fn lock_active_draft(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    draft_id: &str,
) -> Result<i64, String> {
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM volund.import_drafts WHERE public_id::text=$1 FOR UPDATE",
    )
    .bind(draft_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| format!("cannot lock ZIP draft: {error}"))?;
    // now() is the transaction start, potentially before a long lock wait.
    sqlx::query_scalar(
        "SELECT id FROM volund.import_drafts WHERE id=$1 \
        AND expires_at>clock_timestamp() AND status IN \
        ('draft','uploading','uploaded','review_ready','reviewed','failed')",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("cannot validate ZIP draft: {error}"))?
    .ok_or_else(|| "zip_draft_no_longer_active".to_owned())
}

async fn already_published(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    draft_internal: i64,
    item_id: &str,
) -> Result<bool, String> {
    sqlx::query_scalar(
        "SELECT archive_expanded_at IS NOT NULL FROM volund.import_draft_items \
         WHERE import_draft_id=$1 AND public_id::text=$2",
    )
    .bind(draft_internal)
    .bind(item_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| format!("cannot recheck ZIP publication: {error}"))
}

fn cleanup_parts(directory: &Path, item_id: &str) {
    let prefix = format!(".{item_id}-");
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".extract.part") {
                fs::remove_file(entry.path()).ok();
            }
        }
    }
}

fn ensure_regular(entry: &zip::read::ZipFile<'_, File>) -> Result<(), String> {
    if let Some(mode) = entry.unix_mode() {
        let kind = mode & 0o170_000;
        if kind != 0 && kind != 0o100_000 {
            return Err("zip_special_file_refused".to_owned());
        }
    }
    Ok(())
}

fn normalize_archive_path(value: &str) -> Result<String, String> {
    if value.contains('\\') || value.contains('\0') {
        return Err("zip_path_refused".to_owned());
    }
    let path = normalize_library_path(value).map_err(|_| "zip_path_refused".to_owned())?;
    if path.chars().count() > MAX_PATH_CHARS || path.split('/').count() > MAX_DEPTH {
        return Err("zip_path_limit".to_owned());
    }
    Ok(path)
}

fn is_archive_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "zip" | "7z" | "tar" | "gz"
    )
}

fn classify(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "step" | "stp" | "iges" | "igs" | "brep" | "f3d" => "cad",
        "stl" | "3mf" | "obj" | "ply" | "gltf" | "glb" => "mesh",
        "pdf" | "xls" | "xlsx" | "csv" | "txt" | "md" | "doc" | "docx" => "document",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" => "image",
        _ => "other",
    }
}

fn category_directory(category: &str) -> &'static str {
    match category {
        "cad" => "CAD",
        "mesh" => "Meshes",
        "document" => "Dokumente",
        "image" => "Bilder",
        _ => "Sonstiges",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zip::write::SimpleFileOptions;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn archive(entries: &[(&str, &[u8])]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "volund-zip-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.zip");
        let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn archive_paths_and_nested_archives_are_strict() {
        assert!(normalize_archive_path("../escape.step").is_err());
        assert!(normalize_archive_path("/absolute.step").is_err());
        assert!(normalize_archive_path("folder\\file.step").is_err());
        assert!(is_archive_path("nested.ZIP"));
        assert_eq!(classify("CAD/main.STEP"), "cad");
    }

    #[tokio::test]
    async fn aborted_waiter_leaves_cleanup_with_the_running_worker() {
        let zip = archive(&[("tiny.step", b"safe")]);
        let root = zip.parent().unwrap().to_path_buf();
        let first = root.join(".attempt-0.extract.part");
        let late = root.join(".attempt-1.extract.part");
        let foreign = root.join(".other-0.extract.part");
        fs::write(&foreign, b"other request").unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(extract_in_background(
            ExtractionParts {
                directory: root.clone(),
                attempt_key: "attempt".to_owned(),
            },
            move |parts| {
                let first = parts.directory.join(".attempt-0.extract.part");
                fs::write(&first, b"first").unwrap();
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                let retained = first.exists();
                fs::write(parts.directory.join(".attempt-1.extract.part"), b"late").unwrap();
                finished_tx.send(retained).unwrap();
                Ok(Vec::new())
            },
        ));
        started_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        assert!(
            finished_rx.await.unwrap(),
            "cleanup must not race the running writer"
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            while first.exists() || late.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(foreign.exists());
        assert!(zip.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zip_extraction_is_bounded_and_removes_failed_parts() {
        let safe = archive(&[("CAD/main.step", b"safe")]);
        let root = safe.parent().unwrap();
        let extracted = inspect_and_extract(&safe, root, "safe-item").unwrap();
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].original_path, "CAD/main.step");
        fs::remove_file(&extracted[0].part_path).unwrap();
        fs::remove_dir_all(root).unwrap();

        let traversal = archive(&[("../escape.step", b"bad")]);
        let root = traversal.parent().unwrap();
        assert_eq!(
            inspect_and_extract(&traversal, root, "bad-item").unwrap_err(),
            "zip_path_refused"
        );
        assert_eq!(
            fs::read_dir(root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with("extract.part"))
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zip_refuses_nested_colliding_and_bomb_entries() {
        let nested = archive(&[("inner.zip", b"PK\x03\x04")]);
        let root = nested.parent().unwrap();
        assert_eq!(
            inspect_and_extract(&nested, root, "nested").unwrap_err(),
            "zip_nested_archive_refused"
        );
        fs::remove_dir_all(root).unwrap();
        let collision = archive(&[("CAD/Main.step", b"a"), ("cad/main.step", b"b")]);
        let root = collision.parent().unwrap();
        assert_eq!(
            inspect_and_extract(&collision, root, "collision").unwrap_err(),
            "zip_path_collision"
        );
        fs::remove_dir_all(root).unwrap();
        let zeros = vec![0_u8; 1024 * 1024];
        let bomb = archive(&[("huge.step", &zeros)]);
        let root = bomb.parent().unwrap();
        assert_eq!(
            inspect_and_extract(&bomb, root, "bomb").unwrap_err(),
            "zip_ratio_limit"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
