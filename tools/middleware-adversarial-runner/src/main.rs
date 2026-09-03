#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_ITERATIONS: usize = 3;
const DEFAULT_OUTPUT_DIR: &str = "target/adversarial";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeedMode {
    None,
    Go,
    Elixir,
}

#[derive(Debug, Clone, Copy)]
struct Suite {
    name: &'static str,
    cwd: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    seed_mode: SeedMode,
}

const SUITES: &[Suite] = &[
    Suite {
        name: "typescript",
        cwd: ".",
        program: "npm",
        args: &["--prefix", "src/ts", "test"],
        seed_mode: SeedMode::None,
    },
    Suite {
        name: "golang",
        cwd: "src/golang",
        program: "go",
        args: &["test", "-race", "-count=1", "./..."],
        seed_mode: SeedMode::Go,
    },
    Suite {
        name: "rust",
        cwd: "src/rust",
        program: "cargo",
        args: &["test", "--all-features", "--", "--test-threads=16"],
        seed_mode: SeedMode::None,
    },
    Suite {
        name: "gleam",
        cwd: "src/gleam",
        program: "gleam",
        args: &["test"],
        seed_mode: SeedMode::None,
    },
    Suite {
        name: "elixir",
        cwd: "src/elixir",
        program: "mix",
        args: &["test", "--max-cases", "32"],
        seed_mode: SeedMode::Elixir,
    },
    Suite {
        name: "erlang",
        cwd: "src/erlang",
        program: "rebar3",
        args: &["eunit"],
        seed_mode: SeedMode::None,
    },
];

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    language: Option<String>,
    iterations: usize,
    output_dir: PathBuf,
    receipt: PathBuf,
    list: bool,
}

#[derive(Debug)]
struct RunResult {
    suite: &'static str,
    iteration: usize,
    seed: u64,
    command: String,
    log_path: PathBuf,
    duration: Duration,
    status: ExitStatus,
}

fn main() -> ExitCode {
    match execute() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("middleware adversarial runner failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn execute() -> Result<bool, Box<dyn std::error::Error>> {
    let cli = parse_cli(env::args_os().skip(1)).map_err(invalid_input)?;
    if cli.list {
        for suite in SUITES {
            println!("{}", suite.name);
        }
        return Ok(true);
    }

    let root = repository_root()?;
    let suites = selected_suites(cli.language.as_deref()).map_err(invalid_input)?;
    let output_dir = resolve(&root, &cli.output_dir);
    let receipt_path = resolve(&root, &cli.receipt);
    fs::create_dir_all(&output_dir)?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let base_seed = base_seed(&root)?;
    let mut results = Vec::with_capacity(suites.len() * cli.iterations);
    for suite in suites {
        for iteration in 1..=cli.iterations {
            let seed = derive_seed(base_seed, suite.name, iteration);
            let result = run_suite(&root, &output_dir, suite, iteration, seed)?;
            println!(
                "suite={} iteration={} seed={} status={} duration_ms={} log={}",
                result.suite,
                result.iteration,
                result.seed,
                if result.status.success() {
                    "passed"
                } else {
                    "failed"
                },
                result.duration.as_millis(),
                result.log_path.display()
            );
            results.push(result);
        }
    }

    let passed = results.iter().all(|result| result.status.success());
    write_receipt(&receipt_path, &root, base_seed, &results, passed)?;
    Ok(passed)
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn parse_cli(arguments: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    let mut cli = Cli {
        language: None,
        iterations: DEFAULT_ITERATIONS,
        output_dir: PathBuf::from(DEFAULT_OUTPUT_DIR),
        receipt: PathBuf::from(DEFAULT_OUTPUT_DIR).join("receipt.json"),
        list: false,
    };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--language") => {
                cli.language = Some(next_utf8(&mut arguments, "--language")?);
            }
            Some("--iterations") => {
                let value = next_utf8(&mut arguments, "--iterations")?;
                cli.iterations = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --iterations value: {value}"))?;
                if !(1..=100).contains(&cli.iterations) {
                    return Err("--iterations must be between 1 and 100".to_owned());
                }
            }
            Some("--output-dir") => {
                cli.output_dir = PathBuf::from(next_value(&mut arguments, "--output-dir")?);
            }
            Some("--receipt") => {
                cli.receipt = PathBuf::from(next_value(&mut arguments, "--receipt")?);
            }
            Some("--list") => cli.list = true,
            Some("--help" | "-h") => {
                print_help();
                cli.list = true;
            }
            Some(value) => return Err(format!("unknown argument: {value}")),
            None => return Err("arguments must be valid UTF-8".to_owned()),
        }
    }
    Ok(cli)
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<OsString, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn next_utf8(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<String, String> {
    next_value(arguments, option)?
        .into_string()
        .map_err(|_| format!("{option} must be valid UTF-8"))
}

fn print_help() {
    println!(
        "middleware-adversarial-runner\n\n\
         --language <typescript|golang|rust|gleam|elixir|erlang|all>\n\
         --iterations <1-100>\n\
         --output-dir <path>\n\
         --receipt <path>\n\
         --list"
    );
}

fn selected_suites(language: Option<&str>) -> Result<Vec<&'static Suite>, String> {
    match language {
        None | Some("all") => Ok(SUITES.iter().collect()),
        Some(name) => SUITES
            .iter()
            .find(|suite| suite.name == name)
            .map(|suite| vec![suite])
            .ok_or_else(|| format!("unsupported language suite: {name}")),
    }
}

fn repository_root() -> io::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("not inside a Git repository"));
    }
    let root = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(PathBuf::from(root.trim()))
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn base_seed(root: &Path) -> io::Result<u64> {
    if let Ok(value) = env::var("ORES_MIDDLEWARE_STRESS_SEED") {
        return value
            .parse::<u64>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error));
    }
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    let fallback = 0x0A11_CE55_5EED_u64;
    if !output.status.success() {
        return Ok(fallback);
    }
    let sha = String::from_utf8_lossy(&output.stdout);
    let prefix = sha.trim().get(..16).unwrap_or(sha.trim());
    Ok(u64::from_str_radix(prefix, 16).unwrap_or(fallback))
}

fn derive_seed(base: u64, suite: &str, iteration: usize) -> u64 {
    let mut value = base ^ (iteration as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for byte in suite.bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x0000_0100_0000_01B3);
    }
    value
}

fn suite_arguments(suite: &Suite, seed: u64) -> Vec<OsString> {
    let mut arguments = suite.args.iter().map(OsString::from).collect::<Vec<_>>();
    match suite.seed_mode {
        SeedMode::None => {}
        SeedMode::Go => {
            let position = arguments
                .iter()
                .position(|argument| argument.as_os_str() == OsStr::new("./..."))
                .unwrap_or(arguments.len());
            arguments.insert(
                position,
                OsString::from(format!("-shuffle={}", seed % i64::MAX as u64)),
            );
        }
        SeedMode::Elixir => {
            arguments.push(OsString::from("--seed"));
            arguments.push(OsString::from((seed % 1_000_000).to_string()));
        }
    }
    arguments
}

fn run_suite(
    root: &Path,
    output_dir: &Path,
    suite: &'static Suite,
    iteration: usize,
    seed: u64,
) -> io::Result<RunResult> {
    let log_path = output_dir.join(format!("{}-{iteration:03}.log", suite.name));
    let stdout = File::create(&log_path)?;
    let stderr = stdout.try_clone()?;
    let arguments = suite_arguments(suite, seed);
    let command = command_string(suite.program, &arguments);
    let started = Instant::now();
    let status = Command::new(suite.program)
        .args(&arguments)
        .current_dir(root.join(suite.cwd))
        .env("CI", "true")
        .env("RUST_BACKTRACE", "1")
        .env("RUST_TEST_THREADS", "16")
        .env("GOMAXPROCS", "8")
        .env("NODE_OPTIONS", "--unhandled-rejections=strict")
        .env("MIX_ENV", "test")
        .env("ERL_FLAGS", "+S 4:4")
        .env("NO_COLOR", "1")
        .env("CARGO_TERM_COLOR", "never")
        .env("ORES_MIDDLEWARE_STRESS_SEED", seed.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()?;
    Ok(RunResult {
        suite: suite.name,
        iteration,
        seed,
        command,
        log_path: log_path
            .strip_prefix(root)
            .unwrap_or(&log_path)
            .to_path_buf(),
        duration: started.elapsed(),
        status,
    })
}

fn command_string(program: &str, arguments: &[OsString]) -> String {
    std::iter::once(program.to_owned())
        .chain(
            arguments
                .iter()
                .map(|value| value.to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_receipt(
    path: &Path,
    root: &Path,
    base_seed: u64,
    results: &[RunResult],
    passed: bool,
) -> io::Result<()> {
    let commit = git_value(root, &["rev-parse", "HEAD"])
        .unwrap_or_else(|| "unknown".to_owned());
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut receipt = File::create(path)?;
    writeln!(receipt, "{{")?;
    writeln!(
        receipt,
        "  \"schema\": \"ores-middleware/adversarial-receipt/v1\","
    )?;
    writeln!(receipt, "  \"commit\": \"{}\",", json_escape(&commit))?;
    writeln!(receipt, "  \"timestamp_unix_seconds\": {timestamp},")?;
    writeln!(receipt, "  \"base_seed\": {base_seed},")?;
    writeln!(receipt, "  \"passed\": {passed},")?;
    writeln!(receipt, "  \"runs\": [")?;
    for (index, result) in results.iter().enumerate() {
        let comma = if index + 1 == results.len() { "" } else { "," };
        writeln!(receipt, "    {{")?;
        writeln!(receipt, "      \"suite\": \"{}\",", result.suite)?;
        writeln!(receipt, "      \"iteration\": {},", result.iteration)?;
        writeln!(receipt, "      \"seed\": {},", result.seed)?;
        writeln!(
            receipt,
            "      \"command\": \"{}\",",
            json_escape(&result.command)
        )?;
        writeln!(
            receipt,
            "      \"log\": \"{}\",",
            json_escape(&result.log_path.to_string_lossy())
        )?;
        writeln!(
            receipt,
            "      \"duration_ms\": {},",
            result.duration.as_millis()
        )?;
        writeln!(
            receipt,
            "      \"exit_code\": {},",
            result.status.code().unwrap_or(-1)
        )?;
        writeln!(receipt, "      \"passed\": {}", result.status.success())?;
        writeln!(receipt, "    }}{comma}")?;
    }
    writeln!(receipt, "  ]")?;
    writeln!(receipt, "}}")?;
    Ok(())
}

fn git_value(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", control as u32));
            }
            value => escaped.push(value),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_iterations() {
        let cli = parse_cli(
            ["--language", "rust", "--iterations", "9"]
                .into_iter()
                .map(OsString::from),
        )
        .expect("valid options");
        assert_eq!(cli.language.as_deref(), Some("rust"));
        assert_eq!(cli.iterations, 9);
    }

    #[test]
    fn rejects_zero_iterations_and_unknown_suites() {
        assert!(
            parse_cli(["--iterations", "0"].into_iter().map(OsString::from)).is_err()
        );
        assert!(selected_suites(Some("python")).is_err());
    }

    #[test]
    fn seeds_are_repeatable_and_suite_specific() {
        assert_eq!(derive_seed(42, "rust", 3), derive_seed(42, "rust", 3));
        assert_ne!(derive_seed(42, "rust", 3), derive_seed(42, "golang", 3));
        assert_ne!(derive_seed(42, "rust", 3), derive_seed(42, "rust", 4));
    }

    #[test]
    fn go_and_elixir_receive_replayable_seeds() {
        let go = SUITES
            .iter()
            .find(|suite| suite.name == "golang")
            .expect("go suite");
        let elixir = SUITES
            .iter()
            .find(|suite| suite.name == "elixir")
            .expect("elixir suite");
        let go_args = suite_arguments(go, 123);
        let elixir_args = suite_arguments(elixir, 123);
        assert!(
            go_args
                .iter()
                .any(|value| value.as_os_str() == OsStr::new("-shuffle=123"))
        );
        assert!(elixir_args.windows(2).any(|pair| {
            pair[0].as_os_str() == OsStr::new("--seed")
                && pair[1].as_os_str() == OsStr::new("123")
        }));
    }

    #[test]
    fn json_escape_handles_control_characters() {
        assert_eq!(json_escape("a\n\"b\\"), "a\\n\\\"b\\\\");
    }
}
