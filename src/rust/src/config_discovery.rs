//! Filesystem discovery for ORES dotfile configuration.
//!
//! A server should not have to be told where its own `.ores-mw.toml` is. This
//! module resolves a starting directory using the precedence already shared
//! across the ORES SDKs, then walks up the directory tree and consumes the
//! first matching file it finds.
//!
//! Resolution order, highest priority first:
//!
//! 1. an explicit path argument
//! 2. an explicit starting-directory argument
//! 3. `<ENV>_CONFIG_FILE`
//! 4. `<ENV>_CONFIG_DIR`
//! 5. the process working directory
//!
//! Steps 1 and 3 name a file outright and are used as given: an operator who
//! points at an exact file gets that file or an error, never a surprise from a
//! parent directory. Steps 2, 4 and 5 name a directory, and the upward walk
//! starts there. Blank values are skipped and surrounding whitespace trimmed,
//! matching the shared SDK lookup contract.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Hard ceiling on how far up the tree the walk will look. A repository nested
/// more deeply than this is pathological, and the bound keeps a runaway walk
/// from touching unrelated parts of the filesystem.
pub const MAX_ANCESTORS: usize = 64;

/// Refuse implausibly large documents before reading them into memory.
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// No file of that name exists in the starting directory or any ancestor.
    NotFound {
        file_name: String,
        searched_from: PathBuf,
    },
    /// A path was named explicitly but could not be read.
    Unreadable { path: PathBuf },
    /// The file exceeds `MAX_CONFIG_BYTES`, or is not valid UTF-8, or contains NUL.
    InvalidDocument { path: PathBuf },
    /// The working directory could not be determined and no start was given.
    NoStartingDirectory,
}

impl DiscoveryError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "config_not_found",
            Self::Unreadable { .. } => "config_unreadable",
            Self::InvalidDocument { .. } => "config_invalid_document",
            Self::NoStartingDirectory => "config_no_starting_directory",
        }
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound {
                file_name,
                searched_from,
            } => write!(
                formatter,
                "{}: no {file_name} found in {} or any parent directory",
                self.code(),
                searched_from.display()
            ),
            Self::Unreadable { path } => {
                write!(formatter, "{}: cannot read {}", self.code(), path.display())
            }
            Self::InvalidDocument { path } => write!(
                formatter,
                "{}: {} is not a readable UTF-8 document within {MAX_CONFIG_BYTES} bytes",
                self.code(),
                path.display()
            ),
            Self::NoStartingDirectory => {
                write!(formatter, "{}: no starting directory", self.code())
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// A configuration file found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub path: PathBuf,
    pub source: String,
}

/// Where the search should begin. Separating this from the walk keeps the
/// precedence rules testable without touching the filesystem.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub explicit_file: Option<PathBuf>,
    pub explicit_dir: Option<PathBuf>,
    pub env_file: Option<String>,
    pub env_dir: Option<String>,
    pub working_directory: Option<PathBuf>,
}

/// What the precedence rules selected, before any filesystem access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// An exact file was named. Use it or fail; never walk.
    File(PathBuf),
    /// A directory was named or defaulted. Walk upward from here.
    Directory(PathBuf),
}

fn cleaned(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Strips trailing separators so `/work//` and `/work` behave identically.
fn normalise_dir(value: &Path) -> PathBuf {
    let text = value.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        PathBuf::from("/")
    } else {
        PathBuf::from(trimmed)
    }
}

/// Applies the precedence rules. Pure: performs no filesystem access, so the
/// ordering can be tested exhaustively and compared against the shared
/// cross-SDK lookup fixtures.
///
/// # Errors
/// Returns `NoStartingDirectory` when nothing in the `Origin` names a location.
pub fn resolve_start(origin: &Origin) -> Result<Start, DiscoveryError> {
    if let Some(path) = origin.explicit_file.as_deref()
        && let Some(text) = cleaned(&path.to_string_lossy())
    {
        return Ok(Start::File(PathBuf::from(text)));
    }
    if let Some(dir) = origin.explicit_dir.as_deref()
        && let Some(text) = cleaned(&dir.to_string_lossy())
    {
        return Ok(Start::Directory(normalise_dir(Path::new(text))));
    }
    if let Some(text) = origin.env_file.as_deref().and_then(cleaned) {
        return Ok(Start::File(PathBuf::from(text)));
    }
    if let Some(text) = origin.env_dir.as_deref().and_then(cleaned) {
        return Ok(Start::Directory(normalise_dir(Path::new(text))));
    }
    if let Some(dir) = origin.working_directory.as_deref()
        && let Some(text) = cleaned(&dir.to_string_lossy())
    {
        return Ok(Start::Directory(normalise_dir(Path::new(text))));
    }
    Err(DiscoveryError::NoStartingDirectory)
}

fn read_checked(path: &Path) -> Result<String, DiscoveryError> {
    let metadata = fs::metadata(path).map_err(|_| DiscoveryError::Unreadable {
        path: path.to_path_buf(),
    })?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(DiscoveryError::InvalidDocument {
            path: path.to_path_buf(),
        });
    }
    let source = fs::read_to_string(path).map_err(|_| DiscoveryError::InvalidDocument {
        path: path.to_path_buf(),
    })?;
    if source.as_bytes().contains(&0) {
        return Err(DiscoveryError::InvalidDocument {
            path: path.to_path_buf(),
        });
    }
    Ok(source)
}

/// Walks from `directory` up to the filesystem root, returning the first
/// `file_name` found.
///
/// # Errors
/// Returns `NotFound` when no ancestor within `MAX_ANCESTORS` holds the file.
pub fn walk_up(directory: &Path, file_name: &str) -> Result<Discovered, DiscoveryError> {
    // Resolve symlinks first so the ancestor chain is the real one; fall back to
    // the given path when the directory does not exist yet.
    let start = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    for ancestor in start.ancestors().take(MAX_ANCESTORS) {
        let candidate = ancestor.join(file_name);
        if candidate.is_file() {
            return Ok(Discovered {
                source: read_checked(&candidate)?,
                path: candidate,
            });
        }
    }
    Err(DiscoveryError::NotFound {
        file_name: file_name.to_owned(),
        searched_from: start,
    })
}

/// Resolves a starting point and then finds the configuration file.
///
/// # Errors
/// Propagates resolution, walk and read failures.
pub fn discover(file_name: &str, origin: &Origin) -> Result<Discovered, DiscoveryError> {
    match resolve_start(origin)? {
        Start::File(path) => Ok(Discovered {
            source: read_checked(&path)?,
            path,
        }),
        Start::Directory(directory) => walk_up(&directory, file_name),
    }
}

/// Builds an `Origin` from the process environment.
#[must_use]
pub fn origin_from_env(env_prefix: &str) -> Origin {
    Origin {
        explicit_file: None,
        explicit_dir: None,
        env_file: std::env::var(format!("{env_prefix}_CONFIG_FILE")).ok(),
        env_dir: std::env::var(format!("{env_prefix}_CONFIG_DIR")).ok(),
        working_directory: std::env::current_dir().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ores-mw-discovery-{tag}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn origin_dir(dir: &Path) -> Origin {
        Origin {
            explicit_dir: Some(dir.to_path_buf()),
            ..Origin::default()
        }
    }

    #[test]
    fn walks_up_to_the_nearest_ancestor_holding_the_file() {
        let root = temp_root("walk-up");
        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).expect("dirs");
        fs::write(root.join(".ores-mw.toml"), "schema_version = 1\n").expect("write");

        let found = discover(".ores-mw.toml", &origin_dir(&deep)).expect("discovers upward");
        assert_eq!(
            fs::canonicalize(found.path).expect("canon"),
            fs::canonicalize(root.join(".ores-mw.toml")).expect("canon")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_nearest_file_wins_over_a_further_ancestor() {
        let root = temp_root("nearest");
        let deep = root.join("a/b");
        fs::create_dir_all(&deep).expect("dirs");
        fs::write(root.join(".ores-mw.toml"), "far = true\n").expect("write");
        fs::write(root.join("a/.ores-mw.toml"), "near = true\n").expect("write");

        let found = discover(".ores-mw.toml", &origin_dir(&deep)).expect("discovers");
        assert!(found.source.contains("near = true"), "{}", found.source);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_everywhere_is_a_clear_not_found() {
        let root = temp_root("missing");
        let deep = root.join("x/y");
        fs::create_dir_all(&deep).expect("dirs");
        // A name that cannot exist up the real tree above the temp dir.
        let error =
            discover(".ores-mw-absent-9f3a.toml", &origin_dir(&deep)).expect_err("nothing to find");
        assert_eq!(error.code(), "config_not_found");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_explicit_file_is_used_exactly_and_never_walked_past() {
        let root = temp_root("explicit");
        fs::create_dir_all(root.join("a")).expect("dirs");
        fs::write(root.join(".ores-mw.toml"), "ancestor = true\n").expect("write");
        let exact = root.join("a/named.toml");
        fs::write(&exact, "exact = true\n").expect("write");

        let found = discover(
            ".ores-mw.toml",
            &Origin {
                explicit_file: Some(exact.clone()),
                ..Origin::default()
            },
        )
        .expect("uses the exact file");
        assert!(found.source.contains("exact = true"));

        // A named file that does not exist must fail, not silently fall back
        // to the ancestor copy.
        let error = discover(
            ".ores-mw.toml",
            &Origin {
                explicit_file: Some(root.join("a/absent.toml")),
                ..Origin::default()
            },
        )
        .expect_err("no silent fallback");
        assert_eq!(error.code(), "config_unreadable");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn precedence_matches_the_shared_sdk_lookup_order() {
        let base = Origin {
            explicit_file: Some(PathBuf::from("/explicit.toml")),
            explicit_dir: Some(PathBuf::from("/repo")),
            env_file: Some("/srv/custom.toml".to_owned()),
            env_dir: Some("/etc/app".to_owned()),
            working_directory: Some(PathBuf::from("/work")),
        };
        assert_eq!(
            resolve_start(&base).expect("resolves"),
            Start::File(PathBuf::from("/explicit.toml"))
        );

        let no_file = Origin {
            explicit_file: None,
            ..base.clone()
        };
        assert_eq!(
            resolve_start(&no_file).expect("resolves"),
            Start::Directory(PathBuf::from("/repo"))
        );

        let env_only = Origin {
            explicit_file: None,
            explicit_dir: None,
            ..base.clone()
        };
        assert_eq!(
            resolve_start(&env_only).expect("resolves"),
            Start::File(PathBuf::from("/srv/custom.toml"))
        );

        let dir_env = Origin {
            env_file: None,
            ..env_only.clone()
        };
        assert_eq!(
            resolve_start(&dir_env).expect("resolves"),
            Start::Directory(PathBuf::from("/etc/app"))
        );

        let cwd_only = Origin {
            env_dir: None,
            ..dir_env.clone()
        };
        assert_eq!(
            resolve_start(&cwd_only).expect("resolves"),
            Start::Directory(PathBuf::from("/work"))
        );
    }

    #[test]
    fn blank_values_are_skipped_and_trailing_separators_stripped() {
        let origin = Origin {
            explicit_file: Some(PathBuf::from("   ")),
            explicit_dir: Some(PathBuf::from("")),
            env_file: Some("   ".to_owned()),
            env_dir: Some(" /etc/app ".to_owned()),
            working_directory: Some(PathBuf::from("/work")),
        };
        assert_eq!(
            resolve_start(&origin).expect("resolves"),
            Start::Directory(PathBuf::from("/etc/app"))
        );

        let trailing = Origin {
            working_directory: Some(PathBuf::from("/work//")),
            ..Origin::default()
        };
        assert_eq!(
            resolve_start(&trailing).expect("resolves"),
            Start::Directory(PathBuf::from("/work"))
        );
    }

    #[test]
    fn an_empty_origin_has_no_starting_point() {
        assert_eq!(
            resolve_start(&Origin::default())
                .expect_err("nothing to resolve")
                .code(),
            "config_no_starting_directory"
        );
    }

    #[test]
    fn oversized_documents_are_refused_before_use() {
        let root = temp_root("oversized");
        let big = "#".repeat((MAX_CONFIG_BYTES as usize) + 1);
        fs::write(root.join(".ores-mw.toml"), big).expect("write");
        let error = discover(".ores-mw.toml", &origin_dir(&root)).expect_err("too large");
        assert_eq!(error.code(), "config_invalid_document");
        let _ = fs::remove_dir_all(&root);
    }
}
