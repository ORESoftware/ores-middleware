use std::collections::BTreeSet;
use std::fmt;

const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_TARGETS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestError {
    InvalidDocument,
    UnsupportedVersion,
    InvalidRepositoryMode,
    MissingTarget,
    DuplicateTarget,
    InvalidTarget,
    DisabledTarget,
    ClientTarget,
    NonStackTarget,
    StackConfigMismatch,
}

impl RuntimeManifestError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocument => "invalid_manifest",
            Self::UnsupportedVersion => "unsupported_schema_version",
            Self::InvalidRepositoryMode => "invalid_repository_mode",
            Self::MissingTarget => "runtime_target_missing",
            Self::DuplicateTarget => "duplicate_runtime_target",
            Self::InvalidTarget => "invalid_runtime_target",
            Self::DisabledTarget => "runtime_target_disabled",
            Self::ClientTarget => "runtime_target_is_client",
            Self::NonStackTarget => "runtime_target_not_stack",
            Self::StackConfigMismatch => "runtime_stack_config_mismatch",
        }
    }
}

impl fmt::Display for RuntimeManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for RuntimeManifestError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    name: String,
    role: String,
    enabled: bool,
    middleware: String,
    stack_config: Option<String>,
}

#[derive(Default, Clone)]
struct TargetBuilder {
    name: Option<String>,
    role: Option<String>,
    enabled: Option<bool>,
    middleware: Option<String>,
    stack_config: Option<String>,
}

impl TargetBuilder {
    /// Return a new builder with `key` assigned. A key that was already assigned is
    /// a document error; the builder itself is never mutated in place.
    fn with(self, key: &str, value: &str) -> Result<Self, RuntimeManifestError> {
        match key {
            "name" => Ok(Self {
                name: set_once(self.name, parse_string(value)?)?,
                ..self
            }),
            "role" => Ok(Self {
                role: set_once(self.role, parse_string(value)?)?,
                ..self
            }),
            "enabled" => Ok(Self {
                enabled: set_once(self.enabled, parse_bool(value)?)?,
                ..self
            }),
            "middleware" => Ok(Self {
                middleware: set_once(self.middleware, parse_string(value)?)?,
                ..self
            }),
            "stack_config" => Ok(Self {
                stack_config: set_once(self.stack_config, parse_string(value)?)?,
                ..self
            }),
            _ => Err(RuntimeManifestError::InvalidDocument),
        }
    }

    fn finish(self) -> Result<Target, RuntimeManifestError> {
        let name = self.name.ok_or(RuntimeManifestError::InvalidTarget)?;
        if !valid_target_name(&name) {
            return Err(RuntimeManifestError::InvalidTarget);
        }
        let role = self.role.ok_or(RuntimeManifestError::InvalidTarget)?;
        if !matches!(role.as_str(), "server" | "client") {
            return Err(RuntimeManifestError::InvalidTarget);
        }
        let middleware = self.middleware.ok_or(RuntimeManifestError::InvalidTarget)?;
        if !matches!(
            middleware.as_str(),
            "stack" | "propagation-only" | "disabled"
        ) {
            return Err(RuntimeManifestError::InvalidTarget);
        }
        if self
            .stack_config
            .as_deref()
            .is_some_and(|path| !safe_stack_path(path))
        {
            return Err(RuntimeManifestError::InvalidTarget);
        }
        Ok(Target {
            name,
            role,
            enabled: self.enabled.unwrap_or(true),
            middleware,
            stack_config: self.stack_config,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Section {
    #[default]
    Root,
    Target,
    Other,
}

/// The parser state after some prefix of the document.
///
/// Parsing is a fold: [`ParseState::step`] consumes one line and returns a *new*
/// state (struct-update syntax moves the untouched fields, so no field is ever
/// mutated through a reference). The final state is turned into a
/// [`ParsedManifest`] by [`ParseState::finish`].
#[derive(Default)]
struct ParseState {
    section: Section,
    version: Option<u32>,
    repository_mode: Option<String>,
    default_target: Option<String>,
    targets: Vec<Target>,
    current: Option<TargetBuilder>,
    pending_array_depth: usize,
}

/// The runtime-relevant selection admitted from a manifest document.
struct ParsedManifest {
    version: Option<u32>,
    repository_mode: Option<String>,
    default_target: Option<String>,
    targets: Vec<Target>,
}

impl ParseState {
    fn step(self, raw: &str) -> Result<Self, RuntimeManifestError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(self);
        }
        if self.pending_array_depth > 0 {
            let line = strip_comment(raw)?.trim();
            if line.is_empty() {
                return Ok(self);
            }
            let pending_array_depth = advance_array_depth(line, self.pending_array_depth)?;
            return Ok(Self {
                pending_array_depth,
                ..self
            });
        }
        if trimmed.starts_with('[') {
            let header = strip_comment(raw)?.trim();
            if header == "[[targets]]" {
                let closed = self.close_target()?;
                if closed.targets.len() >= MAX_TARGETS {
                    return Err(RuntimeManifestError::InvalidDocument);
                }
                return Ok(Self {
                    current: Some(TargetBuilder::default()),
                    section: Section::Target,
                    ..closed
                });
            }
            if header.starts_with('[') && header.ends_with(']') {
                let closed = self.close_target()?;
                return Ok(Self {
                    section: Section::Other,
                    ..closed
                });
            }
        }
        if self.section == Section::Other {
            return Ok(self);
        }
        let line = strip_comment(raw)?.trim();
        if line.is_empty() {
            return Ok(self);
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(RuntimeManifestError::InvalidDocument)?;
        self.assign(key.trim(), value.trim())
    }

    fn assign(self, key: &str, value: &str) -> Result<Self, RuntimeManifestError> {
        match self.section {
            Section::Root => match key {
                "schema_version" => Ok(Self {
                    version: set_once(self.version, parse_u32(value)?)?,
                    ..self
                }),
                "repository_mode" => Ok(Self {
                    repository_mode: set_once(self.repository_mode, parse_string(value)?)?,
                    ..self
                }),
                "default_target" => Ok(Self {
                    default_target: set_once(self.default_target, parse_string(value)?)?,
                    ..self
                }),
                "allow_overlapping_roots" => parse_bool(value).map(|_| self),
                _ => Err(RuntimeManifestError::InvalidDocument),
            },
            Section::Target if matches!(key, "roots" | "propagate_headers") => Ok(Self {
                pending_array_depth: start_array(value)?,
                ..self
            }),
            Section::Target => {
                let current = self
                    .current
                    .ok_or(RuntimeManifestError::InvalidDocument)?
                    .with(key, value)?;
                Ok(Self {
                    current: Some(current),
                    ..self
                })
            }
            Section::Other => unreachable!("other sections are skipped before key parsing"),
        }
    }

    /// Close the in-progress target (if any) into the target list, returning the
    /// new state. Ownership of the list moves through the state, so appending here
    /// creates no aliasing and needs no `&mut` parameter.
    fn close_target(self) -> Result<Self, RuntimeManifestError> {
        match self.current {
            None => Ok(self),
            Some(builder) => {
                let target = builder.finish()?;
                let targets = self.targets.into_iter().chain(Some(target)).collect();
                Ok(Self {
                    current: None,
                    targets,
                    ..self
                })
            }
        }
    }

    fn finish(self) -> Result<ParsedManifest, RuntimeManifestError> {
        if self.pending_array_depth != 0 {
            return Err(RuntimeManifestError::InvalidDocument);
        }
        let closed = self.close_target()?;
        Ok(ParsedManifest {
            version: closed.version,
            repository_mode: closed.repository_mode,
            default_target: closed.default_target,
            targets: closed.targets,
        })
    }
}

impl ParsedManifest {
    fn parse(source: &str) -> Result<Self, RuntimeManifestError> {
        source
            .lines()
            .try_fold(ParseState::default(), ParseState::step)?
            .finish()
    }

    fn checked(self) -> Result<Self, RuntimeManifestError> {
        if self.version != Some(1) {
            return Err(RuntimeManifestError::UnsupportedVersion);
        }
        match self.repository_mode.as_deref() {
            Some("server-only" | "hybrid") => {}
            Some("client-only") => return Err(RuntimeManifestError::ClientTarget),
            _ => return Err(RuntimeManifestError::InvalidRepositoryMode),
        }
        let distinct_names: BTreeSet<&str> = self
            .targets
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        if distinct_names.len() != self.targets.len() {
            return Err(RuntimeManifestError::DuplicateTarget);
        }
        Ok(self)
    }

    fn select(&self, target_name: Option<&str>) -> Result<&Target, RuntimeManifestError> {
        let selected_name = target_name
            .or(self.default_target.as_deref())
            .ok_or(RuntimeManifestError::MissingTarget)?;
        if !valid_target_name(selected_name) {
            return Err(RuntimeManifestError::InvalidTarget);
        }
        self.targets
            .iter()
            .find(|target| target.name == selected_name)
            .ok_or(RuntimeManifestError::MissingTarget)
    }
}

impl Target {
    fn admit_stack(&self, expected_stack_config: &str) -> Result<(), RuntimeManifestError> {
        if !self.enabled {
            return Err(RuntimeManifestError::DisabledTarget);
        }
        if self.role != "server" {
            return Err(RuntimeManifestError::ClientTarget);
        }
        if self.middleware != "stack" {
            return Err(RuntimeManifestError::NonStackTarget);
        }
        if self.stack_config.as_deref() != Some(expected_stack_config) {
            return Err(RuntimeManifestError::StackConfigMismatch);
        }
        Ok(())
    }
}

/// Admit the runtime-relevant selection encoded in an already peer-authority-validated
/// `.ores-mw.toml` document.
///
/// The complete manifest remains governed by the independent TypeSpec and Draft 2020-12 JSON
/// Schema contracts plus TJSV. This function intentionally validates only the fields needed by a
/// Rust executable to prove that it selected an enabled server `stack` target and the same stack
/// configuration path that was embedded or otherwise admitted by the caller.
///
/// # Errors
/// Returns a bounded, non-reflective error when the runtime selection is malformed or does not
/// match the caller's expected target/stack path.
pub fn admit_server_stack(
    source: &str,
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<(), RuntimeManifestError> {
    if source.len() > MAX_MANIFEST_BYTES
        || source.as_bytes().contains(&0)
        || !safe_stack_path(expected_stack_config)
    {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    ParsedManifest::parse(source)?
        .checked()?
        .select(target_name)?
        .admit_stack(expected_stack_config)
}

/// Value-in/value-out "assign exactly once": returns the filled slot or a document
/// error if it was already filled.
fn set_once<T>(slot: Option<T>, value: T) -> Result<Option<T>, RuntimeManifestError> {
    match slot {
        Some(_) => Err(RuntimeManifestError::InvalidDocument),
        None => Ok(Some(value)),
    }
}

fn parse_u32(value: &str) -> Result<u32, RuntimeManifestError> {
    value
        .parse::<u32>()
        .map_err(|_| RuntimeManifestError::InvalidDocument)
}

fn parse_bool(value: &str) -> Result<bool, RuntimeManifestError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(RuntimeManifestError::InvalidDocument),
    }
}

fn parse_string(value: &str) -> Result<String, RuntimeManifestError> {
    let value = value
        .strip_prefix('"')
        .and_then(|candidate| candidate.strip_suffix('"'))
        .ok_or(RuntimeManifestError::InvalidDocument)?;
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_control() || character == '\\')
    {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    Ok(value.to_owned())
}

fn strip_comment(line: &str) -> Result<&str, RuntimeManifestError> {
    /// Scanner state: `Open(quoted)` while no unquoted `#` has been seen, or
    /// `CommentAt(index)` once one has.
    enum Scan {
        Open { quoted: bool },
        CommentAt(usize),
    }
    let scan = line.bytes().enumerate().try_fold(
        Scan::Open { quoted: false },
        |scan, (index, byte)| match scan {
            Scan::CommentAt(_) => Ok(scan),
            Scan::Open { quoted } => match byte {
                b'"' => Ok(Scan::Open { quoted: !quoted }),
                b'#' if !quoted => Ok(Scan::CommentAt(index)),
                b'\\' if quoted => Err(RuntimeManifestError::InvalidDocument),
                _ => Ok(Scan::Open { quoted }),
            },
        },
    )?;
    match scan {
        Scan::CommentAt(index) => Ok(&line[..index]),
        Scan::Open { quoted: false } => Ok(line),
        Scan::Open { quoted: true } => Err(RuntimeManifestError::InvalidDocument),
    }
}

fn start_array(value: &str) -> Result<usize, RuntimeManifestError> {
    if !value.starts_with('[') {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    advance_array_depth(value, 0)
}

fn advance_array_depth(line: &str, initial: usize) -> Result<usize, RuntimeManifestError> {
    let (depth, quoted) = line.bytes().try_fold(
        (initial, false),
        |(depth, quoted), byte| -> Result<(usize, bool), RuntimeManifestError> {
            match byte {
                b'"' => Ok((depth, !quoted)),
                b'\\' if quoted => Err(RuntimeManifestError::InvalidDocument),
                b'[' if !quoted => Ok((depth.saturating_add(1), quoted)),
                b']' if !quoted => depth
                    .checked_sub(1)
                    .map(|next| (next, quoted))
                    .ok_or(RuntimeManifestError::InvalidDocument),
                _ => Ok((depth, quoted)),
            }
        },
    )?;
    if quoted {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    Ok(depth)
}

fn valid_target_name(value: &str) -> bool {
    let Some(first) = value.bytes().next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn safe_stack_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.ends_with(".json")
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains("//")
        && value
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "stack"
stack_config = "config/middleware.json"
"#;

    #[test]
    fn admitted_default_target_must_match_embedded_stack_path() {
        assert_eq!(
            admit_server_stack(GOOD, None, "config/middleware.json"),
            Ok(())
        );
        assert_eq!(
            admit_server_stack(GOOD, None, "config/other.json"),
            Err(RuntimeManifestError::StackConfigMismatch)
        );
    }

    #[test]
    fn multiline_arrays_are_source_format_equivalent() {
        let multiline = GOOD.replace(
            "roots = [\"src\"]",
            "roots = [\n  \"src\",\n  \"server\",\n]",
        );
        assert_eq!(
            admit_server_stack(&multiline, None, "config/middleware.json"),
            Ok(())
        );
        let unterminated = GOOD.replace("roots = [\"src\"]", "roots = [\n  \"src\",");
        assert_eq!(
            admit_server_stack(&unterminated, None, "config/middleware.json"),
            Err(RuntimeManifestError::InvalidDocument)
        );
    }

    #[test]
    fn flags_and_env_metadata_are_opaque_to_runtime_target_selection() {
        let enriched = GOOD.replace(
            "\n[[targets]]",
            r#"

[flags2env]
contract = ".cli-flags.toml"
require_audit = true
precedence = "argv-over-env"

[[env]]
name = "display_path"
key = "DISPLAY_PATH"
kind = "string"
required = false
secret = false
default = "C:\\runtime\\path # literal"
description = "metadata with # and \\ escapes is validated by the peer compiler"

[[targets]]"#,
        );
        assert_eq!(
            admit_server_stack(&enriched, None, "config/middleware.json"),
            Ok(())
        );
    }

    #[test]
    fn explicit_target_is_supported_for_hybrid_repositories() {
        let hybrid = GOOD
            .replace("server-only", "hybrid")
            .replace("default_target = \"api\"", "default_target = \"browser\"")
            + r#"
[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "propagation-only"
propagate_headers = ["traceparent"]
"#;
        assert_eq!(
            admit_server_stack(&hybrid, Some("api"), "config/middleware.json"),
            Ok(())
        );
        assert_eq!(
            admit_server_stack(&hybrid, None, "config/middleware.json"),
            Err(RuntimeManifestError::ClientTarget)
        );
    }

    #[test]
    fn disabled_client_and_non_stack_targets_fail_closed() {
        for (needle, replacement, expected) in [
            (
                "role = \"server\"",
                "role = \"client\"",
                RuntimeManifestError::ClientTarget,
            ),
            (
                "middleware = \"stack\"",
                "middleware = \"disabled\"",
                RuntimeManifestError::NonStackTarget,
            ),
        ] {
            let source = GOOD.replace(needle, replacement);
            assert_eq!(
                admit_server_stack(&source, None, "config/middleware.json"),
                Err(expected)
            );
        }
        let disabled = GOOD.replace(
            "middleware = \"stack\"",
            "enabled = false\nmiddleware = \"stack\"",
        );
        assert_eq!(
            admit_server_stack(&disabled, None, "config/middleware.json"),
            Err(RuntimeManifestError::DisabledTarget)
        );
    }

    #[test]
    fn malformed_or_ambiguous_selection_is_rejected() {
        let duplicate = format!(
            "{GOOD}\n[[targets]]{}",
            GOOD.split("[[targets]]").nth(1).unwrap()
        );
        assert_eq!(
            admit_server_stack(&duplicate, None, "config/middleware.json"),
            Err(RuntimeManifestError::DuplicateTarget)
        );
        assert_eq!(
            admit_server_stack(
                &GOOD.replace("schema_version = 1", "schema_version = 2"),
                None,
                "config/middleware.json"
            ),
            Err(RuntimeManifestError::UnsupportedVersion)
        );
        assert_eq!(
            admit_server_stack(GOOD, None, "../config/middleware.json"),
            Err(RuntimeManifestError::InvalidDocument)
        );
    }

    #[test]
    fn errors_do_not_reflect_manifest_or_stack_contents() {
        let marker = "synthetic-secret-never-reflect";
        let source = format!("{GOOD}\n# {marker}\n");
        let error = admit_server_stack(&source, None, "config/other.json").unwrap_err();
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(marker));
        assert!(!rendered.contains("config/other.json"));
    }
}
