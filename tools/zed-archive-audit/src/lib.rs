use flate2::read::GzDecoder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use tar::Archive;

const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_PATH_BYTES: usize = 1_024;

const REQUIRED_SUFFIXES: &[&str] = &[
    ".zpkg.toml",
    "contracts/authority-topology.json",
    "src/rust/Cargo.toml",
    "src/ts/package.json",
    "src/golang/go.mod",
    "src/gleam/gleam.toml",
    "src/elixir/mix.exs",
    "src/erlang/rebar.config",
];

const FORBIDDEN_COMPONENTS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "_build",
    "deps",
    "tmp",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditErrorCode {
    Io,
    ArchiveTooLarge,
    InvalidGzipOrTar,
    TooManyEntries,
    UnpackedArchiveTooLarge,
    InvalidPath,
    DuplicatePath,
    ForbiddenPath,
    UnsupportedEntryType,
    MissingRequiredEntry,
    DuplicateRequiredEntry,
    NonReproducibleArchive,
    InvalidSourceCommit,
    InvalidZedVersion,
}

#[derive(Debug)]
pub struct AuditError {
    code: AuditErrorCode,
    detail: String,
}

impl AuditError {
    #[must_use]
    pub fn code(&self) -> AuditErrorCode {
        self.code
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Display for AuditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.detail)
    }
}

impl Error for AuditError {}

fn failure(code: AuditErrorCode, detail: impl Into<String>) -> AuditError {
    AuditError {
        code,
        detail: detail.into(),
    }
}

pub type Result<T> = std::result::Result<T, AuditError>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSummary {
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub entry_count: usize,
    pub file_count: usize,
    pub unpacked_bytes: u64,
    pub tree_sha256: String,
    pub required_entries: BTreeMap<String, String>,
    pub files: Vec<FileRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceReceipt {
    pub schema: &'static str,
    pub status: &'static str,
    pub source_commit: String,
    pub zed_version: String,
    pub byte_reproducible: bool,
    pub archive: ArchiveSummary,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn canonical_path(path: &Path) -> Result<String> {
    let rendered = path.to_str().ok_or_else(|| {
        failure(
            AuditErrorCode::InvalidPath,
            "archive path is not valid UTF-8",
        )
    })?;
    if rendered.is_empty()
        || rendered.len() > MAX_PATH_BYTES
        || rendered.starts_with('/')
        || rendered.starts_with("./")
        || rendered.contains("//")
        || rendered.contains('\\')
        || rendered.chars().any(char::is_control)
    {
        return Err(failure(
            AuditErrorCode::InvalidPath,
            "archive contains a non-canonical path",
        ));
    }

    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(failure(
                AuditErrorCode::InvalidPath,
                "archive path contains traversal, root, prefix, or current-directory syntax",
            ));
        }
    }
    Ok(rendered.to_owned())
}

fn path_is_forbidden(path: &str) -> bool {
    path.split('/').any(|component| {
        FORBIDDEN_COMPONENTS.contains(&component)
            || component == ".env"
            || component.starts_with(".env.")
            || component.ends_with(".log")
    })
}

fn tree_digest(files: &BTreeMap<String, FileRecord>) -> String {
    let mut digest = Sha256::new();
    for record in files.values() {
        digest.update(record.path.as_bytes());
        digest.update([0]);
        digest.update(record.bytes.to_string().as_bytes());
        digest.update([0]);
        digest.update(record.sha256.as_bytes());
        digest.update([b'\n']);
    }
    format!("{:x}", digest.finalize())
}

pub fn inspect_archive(path: &Path) -> Result<ArchiveSummary> {
    let metadata = fs::metadata(path)
        .map_err(|_| failure(AuditErrorCode::Io, "unable to read archive metadata"))?;
    if !metadata.is_file() {
        return Err(failure(
            AuditErrorCode::Io,
            "archive input is not a regular file",
        ));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(failure(
            AuditErrorCode::ArchiveTooLarge,
            "archive exceeds the 256 MiB compressed boundary",
        ));
    }

    let archive_bytes =
        fs::read(path).map_err(|_| failure(AuditErrorCode::Io, "unable to read archive bytes"))?;
    let decoder = GzDecoder::new(archive_bytes.as_slice());
    let mut archive = Archive::new(decoder);
    let entries = archive.entries().map_err(|_| {
        failure(
            AuditErrorCode::InvalidGzipOrTar,
            "archive is not a readable gzip-compressed tar stream",
        )
    })?;

    let mut seen = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut unpacked_bytes = 0_u64;
    let mut entry_count = 0_usize;

    for entry in entries {
        entry_count += 1;
        if entry_count > MAX_ENTRIES {
            return Err(failure(
                AuditErrorCode::TooManyEntries,
                "archive exceeds the 20,000-entry boundary",
            ));
        }

        let mut entry = entry.map_err(|_| {
            failure(
                AuditErrorCode::InvalidGzipOrTar,
                "archive contains an unreadable entry",
            )
        })?;
        let path = entry.path().map_err(|_| {
            failure(
                AuditErrorCode::InvalidPath,
                "archive contains an invalid encoded path",
            )
        })?;
        let path = canonical_path(path.as_ref())?;
        if !seen.insert(path.clone()) {
            return Err(failure(
                AuditErrorCode::DuplicatePath,
                format!("archive contains duplicate path: {path}"),
            ));
        }
        if path_is_forbidden(&path) {
            return Err(failure(
                AuditErrorCode::ForbiddenPath,
                format!("archive contains excluded path: {path}"),
            ));
        }

        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(failure(
                AuditErrorCode::UnsupportedEntryType,
                format!("archive entry is not a regular file or directory: {path}"),
            ));
        }

        let declared_size = entry.size();
        unpacked_bytes = unpacked_bytes.checked_add(declared_size).ok_or_else(|| {
            failure(
                AuditErrorCode::UnpackedArchiveTooLarge,
                "archive size accounting overflowed",
            )
        })?;
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err(failure(
                AuditErrorCode::UnpackedArchiveTooLarge,
                "archive exceeds the 512 MiB unpacked boundary",
            ));
        }

        let mut content = Vec::with_capacity(usize::try_from(declared_size).unwrap_or(0));
        entry.read_to_end(&mut content).map_err(|_| {
            failure(
                AuditErrorCode::InvalidGzipOrTar,
                format!("unable to read archive entry: {path}"),
            )
        })?;
        if content.len() as u64 != declared_size {
            return Err(failure(
                AuditErrorCode::InvalidGzipOrTar,
                format!("archive entry size mismatch: {path}"),
            ));
        }
        files.insert(
            path.clone(),
            FileRecord {
                path,
                bytes: declared_size,
                sha256: sha256(&content),
            },
        );
    }

    let mut required_entries = BTreeMap::new();
    for suffix in REQUIRED_SUFFIXES {
        let matches: Vec<&String> = files
            .keys()
            .filter(|path| path.as_str() == *suffix || path.ends_with(&format!("/{suffix}")))
            .collect();
        match matches.as_slice() {
            [] => {
                return Err(failure(
                    AuditErrorCode::MissingRequiredEntry,
                    format!("archive is missing required regular-file entry suffix: {suffix}"),
                ));
            }
            [path] => {
                required_entries.insert((*suffix).to_owned(), (*path).clone());
            }
            _ => {
                return Err(failure(
                    AuditErrorCode::DuplicateRequiredEntry,
                    format!(
                        "archive contains multiple regular files for required suffix: {suffix}"
                    ),
                ));
            }
        }
    }

    Ok(ArchiveSummary {
        archive_sha256: sha256(&archive_bytes),
        archive_bytes: metadata.len(),
        entry_count,
        file_count: files.len(),
        unpacked_bytes,
        tree_sha256: tree_digest(&files),
        required_entries,
        files: files.into_values().collect(),
    })
}

pub fn audit_pair(
    first: &Path,
    second: &Path,
    source_commit: &str,
    zed_version: &str,
) -> Result<AcceptanceReceipt> {
    if !valid_lower_hex(source_commit, 40) {
        return Err(failure(
            AuditErrorCode::InvalidSourceCommit,
            "source commit must be a 40-character lowercase hexadecimal SHA",
        ));
    }
    if zed_version.trim().is_empty()
        || zed_version.len() > 128
        || zed_version.chars().any(char::is_control)
    {
        return Err(failure(
            AuditErrorCode::InvalidZedVersion,
            "Zed version is empty, oversized, or contains a control character",
        ));
    }

    let first_bytes =
        fs::read(first).map_err(|_| failure(AuditErrorCode::Io, "unable to read first archive"))?;
    let second_bytes = fs::read(second)
        .map_err(|_| failure(AuditErrorCode::Io, "unable to read second archive"))?;
    if first_bytes != second_bytes {
        return Err(failure(
            AuditErrorCode::NonReproducibleArchive,
            format!(
                "Zed archives differ byte-for-byte: first={} second={}",
                sha256(&first_bytes),
                sha256(&second_bytes)
            ),
        ));
    }

    let archive = inspect_archive(first)?;
    let second_summary = inspect_archive(second)?;
    if archive != second_summary {
        return Err(failure(
            AuditErrorCode::NonReproducibleArchive,
            "Zed archive summaries differ after byte equality check",
        ));
    }

    Ok(AcceptanceReceipt {
        schema: "ores.zed-release-acceptance/v1",
        status: "passed",
        source_commit: source_commit.to_owned(),
        zed_version: zed_version.trim().to_owned(),
        byte_reproducible: true,
        archive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Cursor;
    use tar::{Builder, EntryType, Header};

    const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn finish_archive(
        output: &tempfile::NamedTempFile,
        builder: Builder<GzEncoder<Vec<u8>>>,
    ) {
        let encoder = builder.into_inner().expect("tar finish");
        let bytes = encoder.finish().expect("gzip finish");
        fs::write(output.path(), bytes).expect("write archive");
    }

    fn append_regular_file(
        builder: &mut Builder<GzEncoder<Vec<u8>>>,
        path: &str,
        content: &[u8],
    ) {
        let mut header = Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, path, Cursor::new(content))
            .expect("append entry");
    }

    fn archive(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let output = tempfile::NamedTempFile::new().expect("archive file");
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        builder.mode(tar::HeaderMode::Deterministic);
        for (path, content) in entries {
            append_regular_file(&mut builder, path, content);
        }
        finish_archive(&output, builder);
        output
    }

    fn archive_with_required_directory() -> tempfile::NamedTempFile {
        let output = tempfile::NamedTempFile::new().expect("archive file");
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        builder.mode(tar::HeaderMode::Deterministic);

        for (path, content) in valid_entries().into_iter().skip(1) {
            append_regular_file(&mut builder, path, content);
        }

        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "package/.zpkg.toml",
                Cursor::new(Vec::<u8>::new()),
            )
            .expect("append required-shaped directory");

        finish_archive(&output, builder);
        output
    }

    #[cfg(unix)]
    fn archive_with_non_utf8_path() -> tempfile::NamedTempFile {
        let output = tempfile::NamedTempFile::new().expect("archive file");
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        builder.mode(tar::HeaderMode::Deterministic);

        let mut header = Header::new_gnu();
        let invalid_path = b"package/invalid-\xff-name";
        header.as_mut_bytes()[..invalid_path.len()].copy_from_slice(invalid_path);
        header.set_size(1);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(b"x"))
            .expect("append non-UTF-8 entry");

        finish_archive(&output, builder);
        output
    }

    fn valid_entries() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("package/.zpkg.toml", b"[package]\n"),
            ("package/contracts/authority-topology.json", b"{}\n"),
            ("package/src/rust/Cargo.toml", b"[package]\n"),
            ("package/src/ts/package.json", b"{}\n"),
            ("package/src/golang/go.mod", b"module example\n"),
            ("package/src/gleam/gleam.toml", b"name = \"example\"\n"),
            (
                "package/src/elixir/mix.exs",
                b"defmodule Example do\nend\n",
            ),
            ("package/src/erlang/rebar.config", b"[].\n"),
        ]
    }

    #[test]
    fn deterministic_pair_is_admitted() {
        let first = archive(&valid_entries());
        let second = archive(&valid_entries());
        let receipt = audit_pair(first.path(), second.path(), SOURCE_SHA, "zed 0.2.3")
            .expect("valid archive pair");
        assert_eq!(receipt.status, "passed");
        assert!(receipt.byte_reproducible);
        assert_eq!(receipt.archive.file_count, 8);
        assert_eq!(receipt.archive.required_entries.len(), 8);
        assert_eq!(receipt.archive.archive_sha256.len(), 64);
        assert_eq!(receipt.archive.tree_sha256.len(), 64);
    }

    #[test]
    fn forbidden_generated_output_is_rejected() {
        let mut entries = valid_entries();
        entries.push(("package/target/debug/private-output", b"bad"));
        let candidate = archive(&entries);
        let error = inspect_archive(candidate.path()).expect_err("target must be excluded");
        assert_eq!(error.code(), AuditErrorCode::ForbiddenPath);
    }

    #[test]
    fn duplicate_paths_are_rejected() {
        let mut entries = valid_entries();
        entries.push(("package/src/ts/package.json", b"duplicate"));
        let candidate = archive(&entries);
        let error = inspect_archive(candidate.path()).expect_err("duplicate path must fail");
        assert_eq!(error.code(), AuditErrorCode::DuplicatePath);
    }

    #[test]
    fn required_directory_does_not_satisfy_regular_file_requirement() {
        let candidate = archive_with_required_directory();
        let error = inspect_archive(candidate.path())
            .expect_err("a directory named like a manifest must not satisfy the requirement");
        assert_eq!(error.code(), AuditErrorCode::MissingRequiredEntry);
        assert!(error.detail().contains("required regular-file entry"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_archive_path_is_rejected_without_lossy_normalization() {
        let candidate = archive_with_non_utf8_path();
        let error = inspect_archive(candidate.path()).expect_err("non-UTF-8 path must fail");
        assert_eq!(error.code(), AuditErrorCode::InvalidPath);
        assert_eq!(error.detail(), "archive path is not valid UTF-8");
        assert!(!error.detail().contains('\u{fffd}'));
    }

    #[test]
    fn changed_archive_bytes_are_rejected() {
        let first = archive(&valid_entries());
        let mut changed = valid_entries();
        changed[1].1 = b"{\"changed\":true}\n";
        let second = archive(&changed);
        let error = audit_pair(first.path(), second.path(), SOURCE_SHA, "zed 0.2.3")
            .expect_err("different archives must fail");
        assert_eq!(error.code(), AuditErrorCode::NonReproducibleArchive);
    }

    #[test]
    fn source_and_version_evidence_are_validated() {
        let first = archive(&valid_entries());
        let second = archive(&valid_entries());
        assert_eq!(
            audit_pair(first.path(), second.path(), "main", "zed 0.2.3")
                .expect_err("mutable source ref")
                .code(),
            AuditErrorCode::InvalidSourceCommit
        );
        assert_eq!(
            audit_pair(first.path(), second.path(), SOURCE_SHA, "\n")
                .expect_err("empty version")
                .code(),
            AuditErrorCode::InvalidZedVersion
        );
    }
}
