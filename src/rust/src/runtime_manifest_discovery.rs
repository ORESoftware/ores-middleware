use std::{fs, path::{Path, PathBuf}};

use crate::{RuntimeManifestError, admit_server_stack};

pub const ORES_MW_CONFIG_FILENAME: &str = ".ores-mw.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredMiddlewareManifest {
    pub path: PathBuf,
    pub source: String,
    pub at_repository_root: bool,
}

#[derive(Debug)]
pub enum MiddlewareManifestDiscoveryError {
    CurrentDirectory(std::io::Error),
    NotFound { start: PathBuf },
    Read { path: PathBuf, source: std::io::Error },
    Manifest(RuntimeManifestError),
}

impl std::fmt::Display for MiddlewareManifestDiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentDirectory(error) => write!(formatter, "failed to resolve current directory: {error}"),
            Self::NotFound { start } => write!(formatter, "no {ORES_MW_CONFIG_FILENAME} found from {} to the filesystem root", start.display()),
            Self::Read { path, source } => write!(formatter, "failed to read {}: {source}", path.display()),
            Self::Manifest(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for MiddlewareManifestDiscoveryError {}

fn starting_directory(start: &Path) -> &Path {
    if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    }
}

pub fn discover_middleware_manifest(
    start: impl AsRef<Path>,
) -> Result<DiscoveredMiddlewareManifest, MiddlewareManifestDiscoveryError> {
    let start = starting_directory(start.as_ref());
    let path = start
        .ancestors()
        .map(|directory| directory.join(ORES_MW_CONFIG_FILENAME))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| MiddlewareManifestDiscoveryError::NotFound {
            start: start.to_path_buf(),
        })?;

    let directory = path.parent().unwrap_or(start);
    let at_repository_root = directory.join(".git").is_dir();
    if !at_repository_root {
        tracing::warn!(
            config_path = %path.display(),
            config_directory = %directory.display(),
            "middleware config is not adjacent to a .git directory; using nearest config"
        );
    }

    let source = fs::read_to_string(&path).map_err(|source| {
        MiddlewareManifestDiscoveryError::Read {
            path: path.clone(),
            source,
        }
    })?;

    Ok(DiscoveredMiddlewareManifest {
        path,
        source,
        at_repository_root,
    })
}

pub fn discover_middleware_manifest_from_cwd(
) -> Result<DiscoveredMiddlewareManifest, MiddlewareManifestDiscoveryError> {
    let cwd = std::env::current_dir().map_err(MiddlewareManifestDiscoveryError::CurrentDirectory)?;
    discover_middleware_manifest(cwd)
}

/// Discover, read and validate the nearest `.ores-mw.toml` in one owner-library call.
pub fn admit_nearest_server_stack(
    start: impl AsRef<Path>,
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<DiscoveredMiddlewareManifest, MiddlewareManifestDiscoveryError> {
    let discovered = discover_middleware_manifest(start)?;
    admit_server_stack(&discovered.source, target_name, expected_stack_config)
        .map_err(MiddlewareManifestDiscoveryError::Manifest)?;
    Ok(discovered)
}

pub fn admit_nearest_server_stack_from_cwd(
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<DiscoveredMiddlewareManifest, MiddlewareManifestDiscoveryError> {
    let cwd = std::env::current_dir().map_err(MiddlewareManifestDiscoveryError::CurrentDirectory)?;
    admit_nearest_server_stack(cwd, target_name, expected_stack_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("ores-mw-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn nearest_manifest_wins() {
        let root = scratch("nearest");
        let nested_root = root.join("services/api");
        let cwd = nested_root.join("src");
        fs::create_dir_all(root.join(".git")).expect("git dir");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(root.join(ORES_MW_CONFIG_FILENAME), "root").expect("root config");
        fs::write(nested_root.join(ORES_MW_CONFIG_FILENAME), "nested").expect("nested config");

        let found = discover_middleware_manifest(&cwd).expect("nearest config");
        assert_eq!(found.path, nested_root.join(ORES_MW_CONFIG_FILENAME));
        assert_eq!(found.source, "nested");
        assert!(!found.at_repository_root);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn repository_root_manifest_is_recognized() {
        let root = scratch("root");
        let cwd = root.join("src/bin");
        fs::create_dir_all(root.join(".git")).expect("git dir");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(root.join(ORES_MW_CONFIG_FILENAME), "root").expect("root config");

        let found = discover_middleware_manifest(&cwd).expect("root config");
        assert_eq!(found.path, root.join(ORES_MW_CONFIG_FILENAME));
        assert!(found.at_repository_root);

        fs::remove_dir_all(root).expect("cleanup");
    }
}
