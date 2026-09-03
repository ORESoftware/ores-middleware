use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::Value;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tar::{Builder, EntryType, Header};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn finish_archive(output: &tempfile::NamedTempFile, builder: Builder<GzEncoder<Vec<u8>>>) {
    let encoder = builder.into_inner().expect("finish tar stream");
    let bytes = encoder.finish().expect("finish gzip stream");
    fs::write(output.path(), bytes).expect("write archive fixture");
}

fn append_regular_file(builder: &mut Builder<GzEncoder<Vec<u8>>>, path: &str, content: &[u8]) {
    let mut header = Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, Cursor::new(content))
        .expect("append regular file");
}

fn valid_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("package/.zpkg.toml", b"[package]\n"),
        ("package/contracts/authority-topology.json", b"{}\n"),
        ("package/src/rust/Cargo.toml", b"[package]\n"),
        ("package/src/ts/package.json", b"{}\n"),
        ("package/src/golang/go.mod", b"module example\n"),
        ("package/src/gleam/gleam.toml", b"name = \"example\"\n"),
        ("package/src/elixir/mix.exs", b"defmodule Example do\nend\n"),
        ("package/src/erlang/rebar.config", b"[].\n"),
    ]
}

fn valid_archive() -> tempfile::NamedTempFile {
    let output = tempfile::NamedTempFile::new().expect("archive file");
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);
    builder.mode(tar::HeaderMode::Deterministic);
    for (path, content) in valid_entries() {
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

fn copied_archive(source: &Path) -> tempfile::NamedTempFile {
    let output = tempfile::NamedTempFile::new().expect("archive copy");
    fs::copy(source, output.path()).expect("copy archive bytes");
    output
}

fn run_audit(first: &Path, second: &Path, source_commit: &str, receipt: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ores-zed-archive-audit"))
        .args([
            "--first",
            first.to_str().expect("UTF-8 fixture path"),
            "--second",
            second.to_str().expect("UTF-8 fixture path"),
            "--source-commit",
            source_commit,
            "--zed-version",
            "zed 0.2.3",
            "--receipt",
            receipt.to_str().expect("UTF-8 receipt path"),
        ])
        .output()
        .expect("run archive auditor")
}

fn read_receipt(path: PathBuf) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read receipt")).expect("parse receipt")
}

#[test]
fn cli_writes_a_passed_receipt_for_a_reproducible_pair() {
    let first = valid_archive();
    let second = copied_archive(first.path());
    let directory = tempfile::tempdir().expect("receipt directory");
    let receipt_path = directory.path().join("receipt.json");

    let output = run_audit(first.path(), second.path(), SOURCE_SHA, &receipt_path);
    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));

    let receipt = read_receipt(receipt_path);
    assert_eq!(receipt["schema"], "ores.zed-release-acceptance/v1");
    assert_eq!(receipt["status"], "passed");
    assert_eq!(receipt["sourceCommit"], SOURCE_SHA);
    assert_eq!(receipt["byteReproducible"], true);
    assert_eq!(receipt["archive"]["fileCount"], 8);
}

#[test]
fn cli_stops_when_a_directory_impersonates_a_required_manifest() {
    let first = archive_with_required_directory();
    let second = copied_archive(first.path());
    let directory = tempfile::tempdir().expect("receipt directory");
    let receipt_path = directory.path().join("receipt.json");

    let output = run_audit(first.path(), second.path(), SOURCE_SHA, &receipt_path);
    assert_eq!(output.status.code(), Some(2));

    let receipt = read_receipt(receipt_path);
    assert_eq!(receipt["schema"], "ores.zed-release-acceptance/v1");
    assert_eq!(receipt["status"], "stopped_for_evaluation");
    assert_eq!(receipt["error"]["code"], "missing_required_entry");
    assert!(
        receipt["error"]["detail"]
            .as_str()
            .expect("error detail")
            .contains("required regular-file entry")
    );
}

#[cfg(unix)]
#[test]
fn cli_preserves_non_utf8_path_rejection_in_the_receipt() {
    let first = archive_with_non_utf8_path();
    let second = copied_archive(first.path());
    let directory = tempfile::tempdir().expect("receipt directory");
    let receipt_path = directory.path().join("receipt.json");

    let output = run_audit(first.path(), second.path(), SOURCE_SHA, &receipt_path);
    assert_eq!(output.status.code(), Some(2));

    let receipt = read_receipt(receipt_path);
    assert_eq!(receipt["schema"], "ores.zed-release-acceptance/v1");
    assert_eq!(receipt["status"], "stopped_for_evaluation");
    assert_eq!(receipt["error"]["code"], "invalid_path");
    assert_eq!(
        receipt["error"]["detail"],
        "archive path is not valid UTF-8"
    );
    assert!(
        !receipt["error"]["detail"]
            .as_str()
            .expect("error detail")
            .contains('\u{fffd}')
    );
}

#[test]
fn cli_records_invalid_source_provenance_as_a_stopped_receipt() {
    let first = valid_archive();
    let second = copied_archive(first.path());
    let directory = tempfile::tempdir().expect("receipt directory");
    let receipt_path = directory.path().join("receipt.json");

    let output = run_audit(first.path(), second.path(), "main", &receipt_path);
    assert_eq!(output.status.code(), Some(2));

    let receipt = read_receipt(receipt_path);
    assert_eq!(receipt["status"], "stopped_for_evaluation");
    assert_eq!(receipt["error"]["code"], "invalid_source_commit");
    assert_eq!(
        receipt["error"]["detail"],
        "source commit must be a 40-character lowercase hexadecimal SHA"
    );
}
