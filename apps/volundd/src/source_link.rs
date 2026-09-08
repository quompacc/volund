use std::path::{Path, PathBuf};
use tokio::fs;

// Only discard a newly created link while the database transaction is known
// not to have committed. A cancelled/ambiguous COMMIT must retain both links.
#[derive(Default)]
pub(crate) struct StagedLink(Option<(PathBuf, PathBuf)>);

impl StagedLink {
    pub(crate) async fn create(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        match fs::hard_link(from, to).await {
            Ok(()) => self.0 = Some((from.to_owned(), to.to_owned())),
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists && same_file(from, to) => {}
            Err(error) => return Err(format!("cannot stage lifecycle link: {error}")),
        }
        fs::File::open(to)
            .await
            .map_err(|e| e.to_string())?
            .sync_all()
            .await
            .map_err(|e| e.to_string())?;
        sync_parent(to).await?;
        Ok(())
    }

    pub(crate) fn preserve(&mut self) {
        self.0 = None;
    }
}

pub(crate) async fn sync_parent(path: &Path) -> Result<(), String> {
    fs::File::open(path.parent().ok_or("lifecycle path has no parent")?)
        .await
        .map_err(|e| e.to_string())?
        .sync_all()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::StagedLink;

    #[tokio::test]
    async fn failed_rollback_never_discards_the_remaining_link() {
        let root = std::env::temp_dir().join(format!(
            "volund-link-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let original = root.join("original");
        let staged = root.join("staged");
        std::fs::write(&original, b"only remaining bytes").unwrap();
        let mut guard = StagedLink::default();
        guard.create(&original, &staged).await.unwrap();
        std::fs::remove_file(&original).unwrap();
        drop(guard);
        assert_eq!(std::fs::read(&staged).unwrap(), b"only remaining bytes");
        std::fs::remove_dir_all(root).unwrap();
    }
}

impl Drop for StagedLink {
    fn drop(&mut self) {
        if let Some((from, to)) = &self.0 {
            // Never remove the remaining copy after a failed restoration or
            // after another filesystem actor replaces either path.
            if same_file(from, to) {
                let _ = std::fs::remove_file(to);
            }
        }
    }
}

pub(crate) fn same_file(first: &Path, second: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(a), Ok(b)) = (
            std::fs::symlink_metadata(first),
            std::fs::symlink_metadata(second),
        ) else {
            return false;
        };
        a.is_file() && b.is_file() && a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (first, second);
        false // Deployment is native Debian; do not guess file identity elsewhere.
    }
}
