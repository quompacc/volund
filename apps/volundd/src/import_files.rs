use crate::file_hash::sha256_file;
use crate::import_commit::{ImportCommitError, Item};
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use volund_core::normalize_library_path;

#[derive(Default)]
pub(crate) struct ImportFiles {
    links: Vec<crate::source_link::StagedLink>,
    temporaries: Vec<PathBuf>,
    pub(crate) relocated: Vec<i64>,
}

impl ImportFiles {
    pub(crate) fn preserve(&mut self) {
        for link in &mut self.links {
            link.preserve();
        }
    }
}

impl Drop for ImportFiles {
    fn drop(&mut self) {
        // Roll back unpublished links before removing their temporary anchors.
        self.links.clear();
        for path in &self.temporaries {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) async fn publish_files(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library_root: &Path,
    staging_directory: &Path,
    items: &[Item],
    changes: &mut ImportFiles,
) -> Result<(), ImportCommitError> {
    // Finish only intents from earlier committed operations, before creating any new ones.
    for source in items
        .iter()
        .filter(|item| item.action == "relocate")
        .filter_map(|item| item.matched_source_id)
    {
        crate::source_cleanup::finish_locked(transaction, source)
            .await
            .map_err(ImportCommitError::Storage)?;
    }
    for item in items {
        let staged = staging_directory.join(format!("{}.bin", item.public_id));
        verify_file(&staged, item.byte_size, &item.sha256).await?;
        match item.action.as_str() {
            "create" => {
                let target = secure_target(library_root, &item.target_path).await?;
                copy_without_overwrite(&staged, &target, &item.public_id, changes).await?;
            }
            "relocate" => {
                let source = matched_path(item).await?;
                verify_file(&source, item.byte_size, &item.sha256).await?;
                let target = secure_target(library_root, &item.target_path).await?;
                let source_id = item.matched_source_id.ok_or_else(|| {
                    ImportCommitError::Conflict("matched source disappeared".to_owned())
                })?;
                let mut link = crate::source_link::StagedLink::default();
                link.create(&source, &target)
                    .await
                    .map_err(ImportCommitError::Conflict)?;
                changes.links.push(link);
                crate::source_cleanup::enqueue(
                    transaction,
                    source_id,
                    item.matched_relative_path.as_deref().ok_or_else(|| {
                        ImportCommitError::Conflict("matched path disappeared".to_owned())
                    })?,
                    Some(&item.target_path),
                    &item.sha256,
                    item.byte_size,
                )
                .await
                .map_err(ImportCommitError::Database)?;
                changes.relocated.push(source_id);
            }
            "reuse" => {
                let source = matched_path(item).await?;
                verify_file(&source, item.byte_size, &item.sha256).await?;
            }
            "skip" => {}
            other => {
                return Err(ImportCommitError::BadRequest(format!(
                    "unsupported reviewed action: {other}"
                )));
            }
        }
    }
    Ok(())
}

async fn matched_path(item: &Item) -> Result<PathBuf, ImportCommitError> {
    let root = item.matched_root_path.as_ref().ok_or_else(|| {
        ImportCommitError::Conflict("matched library root disappeared".to_owned())
    })?;
    let relative = item
        .matched_relative_path
        .as_ref()
        .ok_or_else(|| ImportCommitError::Conflict("matched source path disappeared".to_owned()))?;
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(storage_error("resolve matched library root"))?;
    let source = fs::canonicalize(canonical_root.join(relative))
        .await
        .map_err(storage_error("resolve matched source file"))?;
    if !source.starts_with(&canonical_root) {
        return Err(ImportCommitError::BadRequest(
            "matched source escapes its library root".to_owned(),
        ));
    }
    Ok(source)
}

async fn secure_target(root: &Path, relative: &str) -> Result<PathBuf, ImportCommitError> {
    let relative = normalize_library_path(relative)
        .map_err(|message| ImportCommitError::BadRequest(message.to_owned()))?;
    if Path::new(&relative).starts_with(".volund-quarantine") {
        return Err(ImportCommitError::BadRequest(
            "the internal quarantine directory is not an import destination".to_owned(),
        ));
    }
    let target = root.join(&relative);
    let relative_parent = Path::new(&relative).parent().ok_or_else(|| {
        ImportCommitError::BadRequest("target file has no parent directory".to_owned())
    })?;
    let mut parent = root.to_owned();
    for component in relative_parent.components() {
        parent.push(component);
        match fs::symlink_metadata(&parent).await {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(ImportCommitError::Conflict(
                    "an import target parent is not a real directory".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&parent)
                    .await
                    .map_err(storage_error("create import target directory"))?;
            }
            Err(error) => return Err(storage_error("inspect import target directory")(error)),
        }
    }
    let canonical_parent = fs::canonicalize(&parent)
        .await
        .map_err(storage_error("resolve import target directory"))?;
    if !canonical_parent.starts_with(root) {
        return Err(ImportCommitError::BadRequest(
            "import target escapes its library root".to_owned(),
        ));
    }
    Ok(target)
}

async fn verify_file(
    path: &Path,
    expected_bytes: i64,
    expected_hash: &str,
) -> Result<(), ImportCommitError> {
    let expected_bytes = u64::try_from(expected_bytes)
        .map_err(|_| ImportCommitError::Database("stored file size is negative".to_owned()))?;
    let metadata = fs::metadata(path)
        .await
        .map_err(storage_error("inspect import source"))?;
    if !metadata.is_file() || metadata.len() != expected_bytes {
        return Err(ImportCommitError::Conflict(
            "an import source size changed; review again".to_owned(),
        ));
    }
    let owned = path.to_owned();
    let hash = tokio::task::spawn_blocking(move || sha256_file(&owned))
        .await
        .map_err(|error| ImportCommitError::Storage(format!("cannot join hash task: {error}")))?
        .map_err(ImportCommitError::Storage)?;
    if hash.as_str() != expected_hash {
        return Err(ImportCommitError::Conflict(
            "an import source hash changed; review again".to_owned(),
        ));
    }
    Ok(())
}

async fn copy_without_overwrite(
    source: &Path,
    target: &Path,
    item_id: &str,
    changes: &mut ImportFiles,
) -> Result<(), ImportCommitError> {
    let temporary = target.with_file_name(format!(".volund-{item_id}.part"));
    if fs::try_exists(&temporary)
        .await
        .map_err(storage_error("inspect temporary import file"))?
    {
        fs::remove_file(&temporary)
            .await
            .map_err(storage_error("remove stale temporary import file"))?;
    }
    let mut input = fs::File::open(source)
        .await
        .map_err(storage_error("open staged import file"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await
        .map_err(storage_error("create temporary import file"))?;
    changes.temporaries.push(temporary.clone());
    if let Err(error) = tokio::io::copy(&mut input, &mut output).await {
        let _ = fs::remove_file(&temporary).await;
        return Err(ImportCommitError::Storage(format!(
            "cannot copy staged import file: {error}"
        )));
    }
    if let Err(error) = output.sync_all().await {
        let _ = fs::remove_file(&temporary).await;
        return Err(ImportCommitError::Storage(format!(
            "cannot sync imported file: {error}"
        )));
    }
    drop(output);
    let mut link = crate::source_link::StagedLink::default();
    link.create(&temporary, target)
        .await
        .map_err(ImportCommitError::Conflict)?;
    changes.links.push(link);
    Ok(())
}

fn storage_error(context: &'static str) -> impl FnOnce(std::io::Error) -> ImportCommitError {
    move |error| ImportCommitError::Storage(format!("cannot {context}: {error}"))
}
