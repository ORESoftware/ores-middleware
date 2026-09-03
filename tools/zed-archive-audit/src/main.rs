use ores_zed_archive_audit::{AuditError, audit_pair};
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug)]
struct Arguments {
    first: PathBuf,
    second: PathBuf,
    source_commit: String,
    zed_version: String,
    receipt: PathBuf,
}

fn parse_args() -> Result<Arguments, String> {
    let mut first = None;
    let mut second = None;
    let mut source_commit = None;
    let mut zed_version = None;
    let mut receipt = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let value = |args: &mut std::iter::Skip<std::env::Args>, flag: &str| {
            args.next().ok_or_else(|| format!("{flag} requires a value"))
        };
        match argument.as_str() {
            "--first" => first = Some(PathBuf::from(value(&mut args, "--first")?)),
            "--second" => second = Some(PathBuf::from(value(&mut args, "--second")?)),
            "--source-commit" => source_commit = Some(value(&mut args, "--source-commit")?),
            "--zed-version" => zed_version = Some(value(&mut args, "--zed-version")?),
            "--receipt" => receipt = Some(PathBuf::from(value(&mut args, "--receipt")?)),
            "-h" | "--help" => {
                println!(
                    "Usage: ores-zed-archive-audit --first PATH --second PATH \
                     --source-commit SHA --zed-version VERSION --receipt PATH"
                );
                return Err(String::new());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        first: first.ok_or_else(|| "--first is required".to_owned())?,
        second: second.ok_or_else(|| "--second is required".to_owned())?,
        source_commit: source_commit.ok_or_else(|| "--source-commit is required".to_owned())?,
        zed_version: zed_version.ok_or_else(|| "--zed-version is required".to_owned())?,
        receipt: receipt.ok_or_else(|| "--receipt is required".to_owned())?,
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "receipt path must have a parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "unable to create receipt directory".to_owned())?;
    let temporary = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("zed-acceptance-receipt")
    ));
    fs::write(
        &temporary,
        format!(
            "{}\n",
            serde_json::to_string_pretty(value)
                .map_err(|_| "unable to encode receipt".to_owned())?
        ),
    )
    .map_err(|_| "unable to write temporary receipt".to_owned())?;
    fs::rename(&temporary, path).map_err(|_| "unable to commit receipt atomically".to_owned())?;
    Ok(())
}

fn error_json(error: &AuditError) -> serde_json::Value {
    json!({
        "schema": "ores.zed-release-acceptance/v1",
        "status": "stopped_for_evaluation",
        "error": {
            "code": error.code(),
            "detail": error.detail(),
        }
    })
}

fn main() -> ExitCode {
    let arguments = match parse_args() {
        Ok(arguments) => arguments,
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };

    match audit_pair(
        &arguments.first,
        &arguments.second,
        &arguments.source_commit,
        &arguments.zed_version,
    ) {
        Ok(receipt) => {
            let value = match serde_json::to_value(receipt) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!("unable to encode acceptance receipt");
                    return ExitCode::from(1);
                }
            };
            if let Err(error) = write_json(&arguments.receipt, &value) {
                eprintln!("{error}");
                return ExitCode::from(1);
            }
            println!("Zed release acceptance passed");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let value = error_json(&error);
            if let Err(write_error) = write_json(&arguments.receipt, &value) {
                eprintln!("{write_error}");
                return ExitCode::from(1);
            }
            println!("STOPPED_FOR_EVALUATION: {}", error.detail());
            ExitCode::from(2)
        }
    }
}
