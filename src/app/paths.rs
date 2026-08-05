use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const APP_ROOT_OVERRIDE: &str = "BANK_STATEMENT_APP_ROOT";
const APP_DIRECTORY_NAME: &str = "BankStatementFidelityEditor";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunWorkspace {
    pub root: PathBuf,
    pub drafts: PathBuf,
    pub cache: PathBuf,
    pub verification: PathBuf,
    pub audit: PathBuf,
    pub output: PathBuf,
    pub temp: PathBuf,
    pub support: PathBuf,
}

impl AppPaths {
    pub fn discover() -> std::io::Result<Self> {
        let root = std::env::var_os(APP_ROOT_OVERRIDE)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|dir| dir.join(APP_DIRECTORY_NAME)))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "platform local-data directory is unavailable",
                )
            })?;
        let paths = Self::with_root(root);
        paths.ensure()?;
        Ok(paths)
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn audit_dir(&self) -> PathBuf {
        self.root.join("audit")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn documents_dir(&self) -> PathBuf {
        self.root.join("documents")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    pub fn support_dir(&self) -> PathBuf {
        self.root.join("support")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for directory in [
            self.root.clone(),
            self.audit_dir(),
            self.cache_dir(),
            self.documents_dir(),
            self.runs_dir(),
            self.support_dir(),
        ] {
            std::fs::create_dir_all(directory)?;
        }
        Ok(())
    }

    pub fn document_id(&self, document: &Path) -> String {
        let absolute = document.canonicalize().unwrap_or_else(|_| {
            if document.is_absolute() {
                document.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(document)
            }
        });
        let mut digest = Sha256::new();
        digest.update(absolute.to_string_lossy().as_bytes());
        format!("{:x}", digest.finalize())
    }

    pub fn create_run_workspace(&self, document: &Path) -> std::io::Result<RunWorkspace> {
        self.workspace_for(document, uuid::Uuid::new_v4())
    }

    pub fn workspace_for(
        &self,
        document: &Path,
        run_id: uuid::Uuid,
    ) -> std::io::Result<RunWorkspace> {
        let root = self
            .runs_dir()
            .join(self.document_id(document))
            .join(run_id.to_string());
        let workspace = RunWorkspace {
            drafts: root.join("drafts"),
            cache: root.join("cache"),
            verification: root.join("verification"),
            audit: root.join("audit"),
            output: root.join("output"),
            temp: root.join("temp"),
            support: root.join("support"),
            root,
        };
        workspace.ensure()?;
        Ok(workspace)
    }
}

impl RunWorkspace {
    pub fn ensure(&self) -> std::io::Result<()> {
        for directory in [
            self.root.clone(),
            self.drafts.clone(),
            self.cache.clone(),
            self.verification.clone(),
            self.audit.clone(),
            self.output.clone(),
            self.temp.clone(),
            self.support.clone(),
        ] {
            std::fs::create_dir_all(directory)?;
        }
        Ok(())
    }
}

/// Resolves a packaged read-only asset path. If running inside a macOS app
/// bundle (`Contents/MacOS`), assets resolve under `Contents/Resources`.
/// Otherwise they resolve relative to the development working directory.
pub fn resolve_asset_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        return path.to_path_buf();
    }

    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy();
        if exe_str.contains("Contents/MacOS") {
            if let Some(resources) = exe
                .parent()
                .and_then(|path| path.parent())
                .map(|path| path.join("Resources"))
            {
                return resources.join(path);
            }
        }
        if let Some(resources) = exe.parent().map(|path| path.join("resources")) {
            let packaged = resources.join(path);
            if packaged.exists() {
                return packaged;
            }
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
}

/// Resolves the executable directory for co-located packaged libraries.
pub fn resolve_exe_dir() -> PathBuf {
    std::env::current_exe()
        .map(|path| path.parent().unwrap_or(Path::new(".")).to_path_buf())
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_asset_path_absolute_is_stable() {
        let path = Path::new("/absolute/path");
        let resolved = resolve_asset_path(path);
        #[cfg(windows)]
        assert!(resolved.is_absolute());
        #[cfg(not(windows))]
        assert_eq!(resolved, path.to_path_buf());
    }

    #[test]
    fn resolve_asset_path_relative_does_not_fail() {
        let resolved = resolve_asset_path("relative/path");
        assert!(resolved.components().count() > 0);
    }

    #[test]
    fn resolve_executable_directory_does_not_fail() {
        assert!(resolve_exe_dir().components().count() > 0);
    }

    #[test]
    fn run_workspaces_are_document_and_run_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_root(temp.path().join("app"));
        paths.ensure().unwrap();
        let first_document = temp.path().join("first.pdf");
        let second_document = temp.path().join("second.pdf");
        std::fs::write(&first_document, b"first").unwrap();
        std::fs::write(&second_document, b"second").unwrap();

        let first_run = paths.create_run_workspace(&first_document).unwrap();
        let second_run = paths.create_run_workspace(&first_document).unwrap();
        let other_document = paths.create_run_workspace(&second_document).unwrap();

        assert_ne!(first_run.root, second_run.root);
        assert_ne!(first_run.root, other_document.root);
        for directory in [
            first_run.drafts,
            first_run.cache,
            first_run.verification,
            first_run.audit,
            first_run.output,
            first_run.temp,
            first_run.support,
        ] {
            assert!(directory.is_dir());
            assert!(directory.starts_with(paths.root()));
        }
    }

    #[test]
    fn absolute_document_identity_is_independent_of_working_directory() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_root(temp.path().join("app"));
        let document = temp.path().join("statement.pdf");
        std::fs::write(&document, b"fixture").unwrap();
        let first = paths.document_id(&document);
        let second = paths.document_id(&document);
        assert_eq!(first, second);
    }
}
