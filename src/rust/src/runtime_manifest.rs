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

#[derive(Default)]
struct TargetBuilder {
    name: Option<String>,
    role: Option<String>,
    enabled: Option<bool>,
    middleware: Option<String>,
    stack_config: Option<String>,
}

impl TargetBuilder {
    fn assign(&mut self, key: &str, value: &str) -> Result<(), RuntimeManifestError> {
        match key {
            "name" => set_once(&mut self.name, parse_string(value)?),
            "role" => set_once(&mut self.role, parse_string(value)?),
            "enabled" => set_once(&mut self.enabled, parse_bool(value)?),
            "middleware" => set_once(&mut self.middleware, parse_string(value)?),
            "stack_config" => set_once(&mut self.stack_config, parse_string(value)?),
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
        if !matches!(middleware.as_str(), "stack" | "propagation-only" | "disabled") {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Root,
    Target,
    Other,
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

    let mut section = Section::Root;
    let mut version = None;
    let mut repository_mode = None;
    let mut default_target = None;
    let mut targets = Vec::new();
    let mut current = None;
    let mut pending_array_depth = 0_usize;

    for raw in source.lines() {
        let line = strip_comment(raw)?.trim();
        if line.is_empty() {
            continue;
        }
        if pending_array_depth > 0 {
            pending_array_depth = advance_array_depth(line, pending_array_depth)?;
            continue;
        }
        if line == "[[targets]]" {
            finish_target(&mut current, &mut targets)?;
            if targets.len() >= MAX_TARGETS {
                return Err(RuntimeManifestError::InvalidDocument);
            }
            current = Some(TargetBuilder::default());
            section = Section::Target;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            finish_target(&mut current, &mut targets)?;
            section = Section::Other;
            continue;
        }

        let (key, value) = line
            .split_once('=')
            .ok_or(RuntimeManifestError::InvalidDocument)?;
        let key = key.trim();
        let value = value.trim();
        match section {
            Section::Root => match key {
                "schema_version" => set_once(&mut version, parse_u32(value)?),
                "repository_mode" => set_once(&mut repository_mode, parse_string(value)?),
                "default_target" => set_once(&mut default_target, parse_string(value)?),
                "allow_overlapping_roots" => {
                    let _ = parse_bool(value)?;
                    Ok(())
                }
                _ => Err(RuntimeManifestError::InvalidDocument),
            }?,
            Section::Target if matches!(key, "roots" | "propagate_headers") => {
                pending_array_depth = start_array(value)?;
            }
            Section::Target => current
                .as_mut()
                .ok_or(RuntimeManifestError::InvalidDocument)?
                .assign(key, value)?,
            Section::Other => {}
        }
    }
    if pending_array_depth != 0 {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    finish_target(&mut current, &mut targets)?;

    if version != Some(1) {
        return Err(RuntimeManifestError::UnsupportedVersion);
    }
    match repository_mode.as_deref() {
        Some("server-only" | "hybrid") => {}
        Some("client-only") => return Err(RuntimeManifestError::ClientTarget),
        _ => return Err(RuntimeManifestError::InvalidRepositoryMode),
    }

    let mut names = BTreeSet::new();
    for target in &targets {
        if !names.insert(target.name.as_str()) {
            return Err(RuntimeManifestError::DuplicateTarget);
        }
    }

    let selected_name = target_name
        .or(default_target.as_deref())
        .ok_or(RuntimeManifestError::MissingTarget)?;
    if !valid_target_name(selected_name) {
        return Err(RuntimeManifestError::InvalidTarget);
    }
    let selected = targets
        .iter()
        .find(|target| target.name == selected_name)
        .ok_or(RuntimeManifestError::MissingTarget)?;
    if !selected.enabled {
        return Err(RuntimeManifestError::DisabledTarget);
    }
    if selected.role != "server" {
        return Err(RuntimeManifestError::ClientTarget);
    }
    if selected.middleware != "stack" {
        return Err(RuntimeManifestError::NonStackTarget);
    }
    if selected.stack_config.as_deref() != Some(expected_stack_config) {
        return Err(RuntimeManifestError::StackConfigMismatch);
    }
    Ok(())
}

fn finish_target(
    current: &mut Option<TargetBuilder>,
    targets: &mut Vec<Target>,
) -> Result<(), RuntimeManifestError> {
    if let Some(builder) = current.take() {
        targets.push(builder.finish()?);
    }
    Ok(())
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), RuntimeManifestError> {
    if slot.is_some() {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    *slot = Some(value);
    Ok(())
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
    let mut quoted = false;
    for (index, byte) in line.bytes().enumerate() {
        if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'#' && !quoted {
            return Ok(&line[..index]);
        } else if byte == b'\\' && quoted {
            return Err(RuntimeManifestError::InvalidDocument);
        }
    }
    if quoted {
        Err(RuntimeManifestError::InvalidDocument)
    } else {
        Ok(line)
    }
}

fn start_array(value: &str) -> Result<usize, RuntimeManifestError> {
    if !value.starts_with('[') {
        return Err(RuntimeManifestError::InvalidDocument);
    }
    advance_array_depth(value, 0)
}

fn advance_array_depth(line: &str, initial: usize) -> Result<usize, RuntimeManifestError> {
    let mut depth = initial;
    let mut quoted = false;
    for byte in line.bytes() {
        match byte {
            b'"' => quoted = !quoted,
            b'\\' if quoted => return Err(RuntimeManifestError::InvalidDocument),
            b'[' if !quoted => depth = depth.saturating_add(1),
            b']' if !quoted => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(RuntimeManifestError::InvalidDocument)?;
            }
            _ => {}
        }
    }
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
