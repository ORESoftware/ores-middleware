use ores_contract_parity::{Discrepancy, run, write_report};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn parse_args() -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut root = env::current_dir().map_err(|error| error.to_string())?;
    let mut report = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--root requires a path".to_owned())?,
                );
            }
            "--report" => {
                report = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--report requires a path".to_owned())?,
                ));
            }
            "-h" | "--help" => {
                println!(
                    "Usage: ores-contract-parity [--root PATH] [--report PATH]\n\
                     Compare independent TypeSpec and JSON Schema authorities."
                );
                return Err(String::new());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok((root, report))
}

fn report_path(root: &Path, explicit: Option<PathBuf>, has_findings: bool) -> PathBuf {
    if let Some(path) = explicit {
        return if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
    }
    let folder = if has_findings {
        "target/discrepancies"
    } else {
        "target/audit"
    };
    root.join(folder).join("docs-serving-contract-parity.json")
}

fn main() -> ExitCode {
    let (root, explicit_report) = match parse_args() {
        Ok(arguments) => arguments,
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };

    let discrepancies = match run(&root) {
        Ok(findings) => findings,
        Err(error) => vec![Discrepancy::new(
            "contract-check-failure",
            error.to_string(),
        )],
    };
    let output = report_path(&root, explicit_report, !discrepancies.is_empty());
    if let Err(error) = write_report(&output, &discrepancies) {
        eprintln!(
            "failed to write parity report {}: {error}",
            output.display()
        );
        return ExitCode::from(1);
    }

    if discrepancies.is_empty() {
        println!("peer contract parity passed; report={}", output.display());
        ExitCode::SUCCESS
    } else {
        println!(
            "STOPPED_FOR_EVALUATION: {} discrepancy(s); report={}",
            discrepancies.len(),
            output.display()
        );
        for item in &discrepancies {
            println!("- {}: {}", item.fingerprint, item.detail);
        }
        ExitCode::from(2)
    }
}
