use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct PublishedFile {
    destination: PathBuf,
    backup: Option<tempfile::NamedTempFile>,
}

/// Coordinates a group of staged file publications.
///
/// Each destination is backed up before replacement. Unless [`commit`](Self::commit)
/// is called, dropping the barrier restores every prior destination in reverse
/// publication order and removes destinations that did not previously exist.
#[derive(Debug, Default)]
pub struct FileCommitBarrier {
    published: Vec<PublishedFile>,
    committed: bool,
}

pub fn staging_path(
    directory: &Path,
    prefix: &str,
    suffix: &str,
) -> io::Result<tempfile::TempPath> {
    std::fs::create_dir_all(directory)?;
    let temporary = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(directory)?
        .into_temp_path();
    std::fs::remove_file(&temporary)?;
    Ok(temporary)
}

impl FileCommitBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&mut self, staged: &Path, destination: &Path) -> io::Result<()> {
        let staged_metadata = std::fs::metadata(staged)?;
        if !staged_metadata.is_file() || staged_metadata.len() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "staged artifact is missing, empty, or not a file: {}",
                    staged.display()
                ),
            ));
        }

        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let backup = if destination.exists() {
            Some(synced_temporary_copy(destination, parent)?)
        } else {
            None
        };
        let replacement = synced_temporary_copy(staged, parent)?;
        replacement
            .persist(destination)
            .map_err(|error| error.error)?;
        self.published.push(PublishedFile {
            destination: destination.to_path_buf(),
            backup,
        });
        Ok(())
    }

    pub fn commit(mut self) {
        self.committed = true;
    }

    fn rollback(&mut self) {
        for published in self.published.iter_mut().rev() {
            let result = if let Some(backup) = published.backup.as_ref() {
                let parent = published
                    .destination
                    .parent()
                    .unwrap_or_else(|| Path::new("."));
                synced_temporary_copy(backup.path(), parent).and_then(|replacement| {
                    replacement
                        .persist(&published.destination)
                        .map(|_| ())
                        .map_err(|error| error.error)
                })
            } else if published.destination.exists() {
                std::fs::remove_file(&published.destination)
            } else {
                Ok(())
            };

            if let Err(error) = result {
                tracing::error!(
                    "[commit-barrier] failed to restore {}: {}",
                    published.destination.display(),
                    error
                );
            }
        }
    }
}

impl Drop for FileCommitBarrier {
    fn drop(&mut self) {
        if !self.committed {
            self.rollback();
        }
    }
}

fn synced_temporary_copy(source: &Path, directory: &Path) -> io::Result<tempfile::NamedTempFile> {
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    std::fs::copy(source, temporary.path())?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rollback_restores_existing_destination_and_removes_new_destination() -> io::Result<()> {
        let dir = tempdir()?;
        let existing = dir.path().join("existing.pdf");
        let created = dir.path().join("created.json");
        let staged_pdf = dir.path().join("staged.pdf");
        let staged_json = dir.path().join("staged.json");
        std::fs::write(&existing, b"prior bytes")?;
        std::fs::write(&staged_pdf, b"new pdf bytes")?;
        std::fs::write(&staged_json, b"new history bytes")?;

        {
            let mut barrier = FileCommitBarrier::new();
            barrier.publish(&staged_pdf, &existing)?;
            barrier.publish(&staged_json, &created)?;
            assert_eq!(std::fs::read(&existing)?, b"new pdf bytes");
            assert_eq!(std::fs::read(&created)?, b"new history bytes");
        }

        assert_eq!(std::fs::read(&existing)?, b"prior bytes");
        assert!(!created.exists());
        Ok(())
    }

    #[test]
    fn commit_keeps_every_published_artifact() -> io::Result<()> {
        let dir = tempdir()?;
        let destination = dir.path().join("output.pdf");
        let staged = dir.path().join("staged.pdf");
        std::fs::write(&destination, b"prior bytes")?;
        std::fs::write(&staged, b"committed bytes")?;

        let mut barrier = FileCommitBarrier::new();
        barrier.publish(&staged, &destination)?;
        barrier.commit();

        assert_eq!(std::fs::read(&destination)?, b"committed bytes");
        Ok(())
    }

    #[test]
    fn invalid_stage_never_changes_destination() -> io::Result<()> {
        let dir = tempdir()?;
        let destination = dir.path().join("output.pdf");
        let empty_stage = dir.path().join("empty.pdf");
        std::fs::write(&destination, b"prior bytes")?;
        std::fs::write(&empty_stage, b"")?;

        let mut barrier = FileCommitBarrier::new();
        assert!(barrier.publish(&empty_stage, &destination).is_err());
        assert_eq!(std::fs::read(&destination)?, b"prior bytes");
        Ok(())
    }
}
