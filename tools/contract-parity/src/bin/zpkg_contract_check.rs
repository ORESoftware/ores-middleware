use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EXPECTED_ZED_TEST_SCRIPT: &str = "python3 scripts/audit.py --receipt target/audit/receipt.json && npm run contracts:polyglot-generate && npm run contracts:generated-check";
const EXPECTED_INSTALLED_SMOKE_TEST: &str = "python3 \"$ZED_PKG_TEST_TARGET/target/package/scripts/installed_package_smoke.py\" --root \"$ZED_PKG_TEST_TARGET\"";
const POLYGLOT_COMMAND: &str = "cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml --bin persistence_codegen -- --output-root target/schema-convergence --report target/schema-convergence/receipt.json";
const RUST_ZPKG_COMMAND: &str = "cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml --bin zpkg_contract_check -- --root .";

const EXPECTED_TARGETS: &[(&str, &str, &str, Option<&str>)] = &[
    ("repository", ".", "none", None),
    ("rust", "src/rust", "rust", Some("ores-middleware-rust")),
    (
        "typescript",
        "src/ts",
        "node",
        Some("ores-middleware-typescript"),
    ),
    (
        "golang",
        "src/golang",
        "go",
        Some("ores-middleware-golang"),
    ),
    (
        "gleam",
        "src/gleam",
        "none",
        Some("ores-middleware-gleam"),
    ),
    (
        "elixir",
        "src/elixir",
        "none",
        Some("ores-middleware-elixir"),
    ),
    (
        "erlang",
        "src/erlang",
        "none",
        Some("ores-middleware-erlang"),
    ),
];

const EXPECTED_OUTPUTS: &[&str] = &[
    "target/rust",
    "target/ts",
    "target/golang",
    "target/gleam",
    "target/elixir",
    "target/erlang",
    "target/package",
];

const EXPECTED_WORKSPACE_SCRIPTS: &[(&str, &str)] = &[
    (
        "audit",
        "python3 scripts/audit.py --receipt target/audit/receipt.json",
    ),
    (
        "contracts:compile",
        "tsp compile contracts/typespec --output-dir target/contracts/typespec && tsp compile contracts/docs-serving.tsp --no-emit && tsp compile contracts/persistence/idempotency-record.tsp --no-emit && tsp compile contracts/rate-limit-v2/typespec --no-emit",
    ),
    (
        "contracts:cross-translate",
        "python3 scripts/cross_translate.py",
    ),
    ("contracts:polyglot-generate", POLYGLOT_COMMAND),
    (
        "contracts:generated-check",
        "node scripts/validate-generated-polyglot.mjs",
    ),
    ("persistence:check", "python3 scripts/orm_matrix_gate.py"),
    ("zpkg:check", "python3 scripts/check_zpkg.py"),
    ("zpkg:check:rust", RUST_ZPKG_COMMAND),
];

const REQUIRED_PATHS: &[&str] = &[
    "tools/contract-parity/src/bin/persistence_codegen.rs",
    "tools/contract-parity/src/bin/zpkg_contract_check.rs",
    "scripts/check_zpkg.py",
    "scripts/validate-generated-polyglot.mjs",
    "scripts/orm_matrix_gate.py",
    "scripts/test_orm_matrix_gate.py",
    "scripts/orm_catalog_gate.py",
    "scripts/subprocess_capture.py",
    "scripts/build_targets.py",
    "scripts/cross_translate.py",
    "scripts/installed_package_smoke.py",
    ".github/workflows/persistence-convergence.yml",
    ".github/workflows/zed-release-acceptance.yml",
    ".github/workflows/zpkg-contract-rust.yml",
];

const DIGESTED_SOURCES: &[&str] = &[
    ".zpkg.toml",
    "package.json",
    "contracts/authority-topology.json",
    "scripts/check_zpkg.py",
    "tools/contract-parity/src/bin/zpkg_contract_check.rs",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FindingCode {
    Io,
    ManifestSyntax,
    PackageIdentity,
    PackageLanguage,
    ScriptContract,
    TargetSet,
    TargetField,
    TargetDirectory,
    BuildContract,
    WorkspaceScript,
    ComparisonContract,
    RequiredPath,
    InstalledSmoke,
    PublishSmoke,
    JsonSyntax,
    AuthorityTopology,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    code: FindingCode,
    path: String,
    detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    schema: &'static str,
    status: &'static str,
    zero_unexplained_findings: bool,
    source_digests: BTreeMap<String, String>,
    findings: Vec<Finding>,
}

type Manifest = BTreeMap<String, BTreeMap<String, String>>;

fn finding(code: FindingCode, path: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding {
        code,
        path: path.into(),
        detail: detail.into(),
    }
}

fn strip_inline_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if character == '"' {
            in_string = true;
        } else if character == '#' {
            return &line[..index];
        }
    }
    line
}

fn bracket_depth(value: &str) -> i32 {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for character in value.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn parse_assignments(section: &str, body: &str) -> Result<BTreeMap<String, String>, Finding> {
    let mut assignments = BTreeMap::new();
    let mut pending: Option<(String, String, i32)> = None;

    for (index, line) in body.lines().enumerate() {
        let clean = strip_inline_comment(line).trim();
        if clean.is_empty() {
            continue;
        }

        if let Some((key, raw, depth)) = pending.as_mut() {
            raw.push('\n');
            raw.push_str(clean);
            *depth += bracket_depth(clean);
            if *depth < 0 {
                return Err(finding(
                    FindingCode::ManifestSyntax,
                    ".zpkg.toml",
                    format!("section [{section}] has an unmatched ] near line {}", index + 1),
                ));
            }
            if *depth == 0 {
                let (key, raw, _) = pending.take().expect("pending assignment exists");
                if assignments.insert(key.clone(), raw).is_some() {
                    return Err(finding(
                        FindingCode::ManifestSyntax,
                        ".zpkg.toml",
                        format!("section [{section}] contains duplicate key {key}"),
                    ));
                }
            }
            continue;
        }

        let Some((key, raw)) = clean.split_once('=') else {
            return Err(finding(
                FindingCode::ManifestSyntax,
                ".zpkg.toml",
                format!("section [{section}] has a non-assignment line: {clean}"),
            ));
        };
        let key = key.trim();
        let raw = raw.trim();
        if key.is_empty() || raw.is_empty() {
            return Err(finding(
                FindingCode::ManifestSyntax,
                ".zpkg.toml",
                format!("section [{section}] has an empty key or value"),
            ));
        }
        let depth = bracket_depth(raw);
        if depth < 0 {
            return Err(finding(
                FindingCode::ManifestSyntax,
                ".zpkg.toml",
                format!("section [{section}] has an unmatched ] for key {key}"),
            ));
        }
        if depth > 0 {
            pending = Some((key.to_owned(), raw.to_owned(), depth));
        } else if assignments.insert(key.to_owned(), raw.to_owned()).is_some() {
            return Err(finding(
                FindingCode::ManifestSyntax,
                ".zpkg.toml",
                format!("section [{section}] contains duplicate key {key}"),
            ));
        }
    }

    if let Some((key, _, _)) = pending {
        return Err(finding(
            FindingCode::ManifestSyntax,
            ".zpkg.toml",
            format!("section [{section}] has an unterminated array for key {key}"),
        ));
    }
    Ok(assignments)
}

fn parse_manifest(source: &str) -> Result<Manifest, Finding> {
    let mut bodies: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;

    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') || trimmed.starts_with("[[") || trimmed.len() < 3 {
                return Err(finding(
                    FindingCode::ManifestSyntax,
                    ".zpkg.toml",
                    format!("invalid table header at line {}: {trimmed}", index + 1),
                ));
            }
            let name = &trimmed[1..trimmed.len() - 1];
            if name.is_empty() || name.contains('[') || name.contains(']') {
                return Err(finding(
                    FindingCode::ManifestSyntax,
                    ".zpkg.toml",
                    format!("invalid table name at line {}: {trimmed}", index + 1),
                ));
            }
            if bodies.insert(name.to_owned(), String::new()).is_some() {
                return Err(finding(
                    FindingCode::ManifestSyntax,
                    ".zpkg.toml",
                    format!("duplicate table [{name}]"),
                ));
            }
            current = Some(name.to_owned());
            continue;
        }

        if let Some(name) = current.as_ref() {
            let body = bodies.get_mut(name).expect("current table was inserted");
            body.push_str(line);
            body.push('\n');
        } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Err(finding(
                FindingCode::ManifestSyntax,
                ".zpkg.toml",
                format!("top-level assignment before a table at line {}", index + 1),
            ));
        }
    }

    let mut manifest = BTreeMap::new();
    for (name, body) in bodies {
        manifest.insert(name.clone(), parse_assignments(&name, &body)?);
    }
    Ok(manifest)
}

fn parse_string(raw: &str, path: &str, field: &str) -> Result<String, Finding> {
    serde_json::from_str(raw).map_err(|error| {
        finding(
            FindingCode::ManifestSyntax,
            path,
            format!("{field} must be a basic quoted string: {error}"),
        )
    })
}

fn parse_string_array(raw: &str, path: &str, field: &str) -> Result<Vec<String>, Finding> {
    let mut normalized = raw.trim().to_owned();
    if let Some(close) = normalized.rfind(']') {
        if let Some(comma) = normalized[..close].rfind(',') {
            if normalized[comma + 1..close].trim().is_empty() {
                normalized.remove(comma);
            }
        }
    }
    serde_json::from_str(&normalized).map_err(|error| {
        finding(
            FindingCode::ManifestSyntax,
            path,
            format!("{field} must be an array of basic quoted strings: {error}"),
        )
    })
}

fn required_raw<'a>(
    manifest: &'a Manifest,
    section: &str,
    key: &str,
    findings: &mut Vec<Finding>,
) -> Option<&'a str> {
    let Some(table) = manifest.get(section) else {
        findings.push(finding(
            FindingCode::ManifestSyntax,
            ".zpkg.toml",
            format!("missing table [{section}]"),
        ));
        return None;
    };
    let Some(value) = table.get(key) else {
        findings.push(finding(
            FindingCode::ManifestSyntax,
            ".zpkg.toml",
            format!("missing {section}.{key}"),
        ));
        return None;
    };
    Some(value)
}

fn manifest_string(
    manifest: &Manifest,
    section: &str,
    key: &str,
    findings: &mut Vec<Finding>,
) -> Option<String> {
    let raw = required_raw(manifest, section, key, findings)?;
    match parse_string(raw, ".zpkg.toml", &format!("{section}.{key}")) {
        Ok(value) => Some(value),
        Err(error) => {
            findings.push(error);
            None
        }
    }
}

fn read_text(root: &Path, relative: &str, findings: &mut Vec<Finding>) -> Option<String> {
    match fs::read_to_string(root.join(relative)) {
        Ok(content) => Some(content),
        Err(error) => {
            findings.push(finding(
                FindingCode::Io,
                relative,
                format!("unable to read UTF-8 source: {error}"),
            ));
            None
        }
    }
}

fn parse_json(root: &Path, relative: &str, findings: &mut Vec<Finding>) -> Option<Value> {
    let source = read_text(root, relative, findings)?;
    match serde_json::from_str(&source) {
        Ok(value) => Some(value),
        Err(error) => {
            findings.push(finding(
                FindingCode::JsonSyntax,
                relative,
                format!("invalid JSON: {error}"),
            ));
            None
        }
    }
}

fn validate_manifest(root: &Path, manifest: &Manifest, findings: &mut Vec<Finding>) {
    let org = manifest_string(manifest, "package", "org", findings);
    let name = manifest_string(manifest, "package", "name", findings);
    if org.as_deref() != Some("oresoftware") || name.as_deref() != Some("ores-middleware") {
        findings.push(finding(
            FindingCode::PackageIdentity,
            ".zpkg.toml",
            "package identity must be oresoftware/ores-middleware",
        ));
    }
    if manifest
        .get("package")
        .is_some_and(|package| package.contains_key("language"))
    {
        findings.push(finding(
            FindingCode::PackageLanguage,
            ".zpkg.toml",
            "package.language must remain unset for a polyglot repository",
        ));
    }

    match manifest.get("scripts") {
        Some(scripts) => {
            let keys: BTreeSet<&str> = scripts.keys().map(String::as_str).collect();
            if keys != BTreeSet::from(["test"]) {
                findings.push(finding(
                    FindingCode::ScriptContract,
                    ".zpkg.toml",
                    "Zed 0.2.3 [scripts] must contain exactly the supported test hook",
                ));
            }
            match scripts
                .get("test")
                .map(|raw| parse_string(raw, ".zpkg.toml", "scripts.test"))
            {
                Some(Ok(value)) if value == EXPECTED_ZED_TEST_SCRIPT => {}
                Some(Ok(value)) => findings.push(finding(
                    FindingCode::ScriptContract,
                    ".zpkg.toml",
                    format!("scripts.test mismatch: {value}"),
                )),
                Some(Err(error)) => findings.push(error),
                None => {}
            }
        }
        None => findings.push(finding(
            FindingCode::ScriptContract,
            ".zpkg.toml",
            "missing [scripts] table",
        )),
    }

    let expected_target_sections: BTreeSet<String> = EXPECTED_TARGETS
        .iter()
        .map(|(key, _, _, _)| format!("targets.{key}"))
        .collect();
    let actual_target_sections: BTreeSet<String> = manifest
        .keys()
        .filter(|section| section.starts_with("targets."))
        .cloned()
        .collect();
    if actual_target_sections != expected_target_sections {
        findings.push(finding(
            FindingCode::TargetSet,
            ".zpkg.toml",
            format!(
                "target table set mismatch: expected {expected_target_sections:?}, got {actual_target_sections:?}"
            ),
        ));
    }

    let mut names = BTreeSet::new();
    for &(key, directory, adapter, expected_name) in EXPECTED_TARGETS {
        let section = format!("targets.{key}");
        let actual_directory = manifest_string(manifest, &section, "dir", findings);
        let actual_adapter = manifest_string(manifest, &section, "adapter", findings);
        if actual_directory.as_deref() != Some(directory) {
            findings.push(finding(
                FindingCode::TargetField,
                ".zpkg.toml",
                format!("{section}.dir must be {directory:?}"),
            ));
        }
        if actual_adapter.as_deref() != Some(adapter) {
            findings.push(finding(
                FindingCode::TargetField,
                ".zpkg.toml",
                format!("{section}.adapter must be {adapter:?}"),
            ));
        }
        let actual_name = manifest
            .get(&section)
            .and_then(|target| target.get("name"))
            .map(|raw| parse_string(raw, ".zpkg.toml", &format!("{section}.name")));
        match (expected_name, actual_name) {
            (None, None) => {}
            (None, Some(Ok(_))) => findings.push(finding(
                FindingCode::TargetField,
                ".zpkg.toml",
                "targets.repository must publish under package.name and omit name",
            )),
            (None, Some(Err(error))) => findings.push(error),
            (Some(expected), Some(Ok(actual))) if actual == expected => {
                if !names.insert(actual.clone()) {
                    findings.push(finding(
                        FindingCode::TargetField,
                        ".zpkg.toml",
                        format!("duplicate target package name {actual:?}"),
                    ));
                }
            }
            (Some(expected), Some(Ok(actual))) => findings.push(finding(
                FindingCode::TargetField,
                ".zpkg.toml",
                format!("{section}.name must be {expected:?}, got {actual:?}"),
            )),
            (Some(expected), None) => findings.push(finding(
                FindingCode::TargetField,
                ".zpkg.toml",
                format!("{section}.name must be {expected:?}"),
            )),
            (Some(_), Some(Err(error))) => findings.push(error),
        }
        if directory != "." && !root.join(directory).is_dir() {
            findings.push(finding(
                FindingCode::TargetDirectory,
                directory,
                "target directory does not exist",
            ));
        }
    }

    if manifest_string(manifest, "build", "command", findings).as_deref()
        != Some("python3 scripts/build_targets.py")
    {
        findings.push(finding(
            FindingCode::BuildContract,
            ".zpkg.toml",
            "build.command must use the checked-in polyglot build orchestrator",
        ));
    }
    if let Some(raw) = required_raw(manifest, "build", "outputs", findings) {
        match parse_string_array(raw, ".zpkg.toml", "build.outputs") {
            Ok(outputs) => {
                let actual: BTreeSet<String> = outputs.into_iter().collect();
                let expected: BTreeSet<String> =
                    EXPECTED_OUTPUTS.iter().map(|value| (*value).to_owned()).collect();
                if actual != expected {
                    findings.push(finding(
                        FindingCode::BuildContract,
                        ".zpkg.toml",
                        format!("build.outputs mismatch: expected {expected:?}, got {actual:?}"),
                    ));
                }
            }
            Err(error) => findings.push(error),
        }
    }

    if manifest_string(manifest, "publish", "smoke_test", findings).as_deref()
        != Some(EXPECTED_INSTALLED_SMOKE_TEST)
    {
        findings.push(finding(
            FindingCode::PublishSmoke,
            ".zpkg.toml",
            "publish.smoke_test must execute the installed build-output closure",
        ));
    }
}

fn validate_workspace(root: &Path, findings: &mut Vec<Finding>) {
    let Some(workspace) = parse_json(root, "package.json", findings) else {
        return;
    };
    let Some(scripts) = workspace.get("scripts").and_then(Value::as_object) else {
        findings.push(finding(
            FindingCode::WorkspaceScript,
            "package.json",
            "scripts must be an object",
        ));
        return;
    };
    for &(name, expected) in EXPECTED_WORKSPACE_SCRIPTS {
        if scripts.get(name).and_then(Value::as_str) != Some(expected) {
            findings.push(finding(
                FindingCode::WorkspaceScript,
                "package.json",
                format!("scripts.{name} must be {expected:?}"),
            ));
        }
    }
    let compare = scripts
        .get("contracts:compare")
        .and_then(Value::as_str)
        .unwrap_or_default();
    for fragment in [
        "contracts:polyglot-generate",
        "contracts:generated-check",
        "contracts:cross-translate",
    ] {
        if !compare.contains(fragment) {
            findings.push(finding(
                FindingCode::ComparisonContract,
                "package.json",
                format!("scripts.contracts:compare must execute {fragment}"),
            ));
        }
    }
}

fn validate_required_files(root: &Path, findings: &mut Vec<Finding>) {
    for relative in REQUIRED_PATHS {
        if !root.join(relative).is_file() {
            findings.push(finding(
                FindingCode::RequiredPath,
                *relative,
                "missing required convergence/package gate file",
            ));
        }
    }

    if let Some(smoke) = read_text(root, "scripts/installed_package_smoke.py", findings) {
        for fragment in [
            "cross_translate.py",
            "target_root / \"package\"",
            "Rust/TypeScript/Go descriptor parity",
            "Gleam/Elixir/Erlang runtime probes",
        ] {
            if !smoke.contains(fragment) {
                findings.push(finding(
                    FindingCode::InstalledSmoke,
                    "scripts/installed_package_smoke.py",
                    format!("installed package smoke test must retain {fragment:?}"),
                ));
            }
        }
    }
}

fn value_string_array(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToOwned::to_owned))
        .collect()
}

fn validate_topology(root: &Path, findings: &mut Vec<Finding>) {
    let Some(topology) = parse_json(root, "contracts/authority-topology.json", findings) else {
        return;
    };
    let authorities: BTreeSet<String> = topology
        .get("authorities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|authority| {
            authority.get("kind").and_then(Value::as_str) == Some("human-authored")
                && authority.get("topLevel").and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|authority| authority.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();
    let expected_authorities = BTreeSet::from([
        "typespec".to_owned(),
        "json-schema-openapi".to_owned(),
    ]);
    if authorities != expected_authorities {
        findings.push(finding(
            FindingCode::AuthorityTopology,
            "contracts/authority-topology.json",
            "TypeSpec and JSON Schema/OpenAPI must remain human-authored top-level peers",
        ));
    }

    let expected_flows = [
        (
            "typespec",
            vec!["sql-when-applicable", "protobuf", "grpc", "wire-clients"],
        ),
        (
            "json-schema-openapi",
            vec!["interfaces-types", "sql-when-applicable", "write-clients"],
        ),
    ];
    for (authority, expected) in expected_flows {
        let actual = value_string_array(topology.get("flows").and_then(|flows| flows.get(authority)));
        let expected: Vec<String> = expected.into_iter().map(ToOwned::to_owned).collect();
        if actual.as_ref() != Some(&expected) {
            findings.push(finding(
                FindingCode::AuthorityTopology,
                "contracts/authority-topology.json",
                format!("authority flow mismatch for {authority}"),
            ));
        }
    }

    let edges: BTreeSet<(String, String)> = topology
        .get("prohibitedAuthorityEdges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|edge| {
            let edge = edge.as_array()?;
            if edge.len() != 2 {
                return None;
            }
            Some((
                edge[0].as_str()?.to_owned(),
                edge[1].as_str()?.to_owned(),
            ))
        })
        .collect();
    let expected_edges = BTreeSet::from([
        (
            "typespec".to_owned(),
            "json-schema-openapi".to_owned(),
        ),
        (
            "json-schema-openapi".to_owned(),
            "typespec".to_owned(),
        ),
    ]);
    if !expected_edges.is_subset(&edges) {
        findings.push(finding(
            FindingCode::AuthorityTopology,
            "contracts/authority-topology.json",
            "both authority-precedence edges must remain prohibited",
        ));
    }

    let gates: BTreeSet<String> = value_string_array(topology.get("convergenceGates"))
        .unwrap_or_default()
        .into_iter()
        .collect();
    let required_gates: BTreeSet<String> = [
        "cross-translation-witnesses",
        "round-trip-witnesses",
        "sql-catalog-readback-when-applicable",
        "diesel-seaorm-catalog-parity-when-applicable",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect();
    if !required_gates.is_subset(&gates) {
        findings.push(finding(
            FindingCode::AuthorityTopology,
            "contracts/authority-topology.json",
            "translation, round-trip, SQL catalog, and Diesel/SeaORM gates are required",
        ));
    }
    if topology
        .get("onUnexplainedMismatch")
        .and_then(Value::as_str)
        != Some("STOPPED_FOR_EVALUATION")
    {
        findings.push(finding(
            FindingCode::AuthorityTopology,
            "contracts/authority-topology.json",
            "unexplained mismatches must stop for evaluation",
        ));
    }
}

fn validate(root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(source) = read_text(root, ".zpkg.toml", &mut findings) {
        match parse_manifest(&source) {
            Ok(manifest) => validate_manifest(root, &manifest, &mut findings),
            Err(error) => findings.push(error),
        }
    }
    validate_workspace(root, &mut findings);
    validate_required_files(root, &mut findings);
    validate_topology(root, &mut findings);
    findings
}

fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn receipt(root: &Path, findings: Vec<Finding>) -> Receipt {
    let source_digests = DIGESTED_SOURCES
        .iter()
        .filter_map(|relative| {
            sha256_file(&root.join(relative)).map(|digest| ((*relative).to_owned(), digest))
        })
        .collect();
    let passed = findings.is_empty();
    Receipt {
        schema: "ores.zpkg-contract-audit/v1",
        status: if passed {
            "passed"
        } else {
            "stopped_for_evaluation"
        },
        zero_unexplained_findings: passed,
        source_digests,
        findings,
    }
}

fn parse_args() -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut root = env::current_dir().map_err(|error| error.to_string())?;
    let mut receipt_path = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => {
                let value = args.next().ok_or("--root requires a path")?;
                root = PathBuf::from(value);
            }
            "--receipt" => {
                let value = args.next().ok_or("--receipt requires a path")?;
                receipt_path = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                println!(
                    "Usage: zpkg_contract_check [--root PATH] [--receipt PATH]\n\nValidates the repository's polyglot Zed contract and peer-authority topology."
                );
                return Err(String::new());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((root, receipt_path))
}

fn write_receipt(path: &Path, report: &Receipt) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let serialized = serde_json::to_string_pretty(report).map_err(|error| error.to_string())?;
    fs::write(path, format!("{serialized}\n")).map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let (root, receipt_path) = match parse_args() {
        Ok(values) => values,
        Err(error) if error.is_empty() => return ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zpkg contract checker argument error: {error}");
            return ExitCode::from(64);
        }
    };
    let root = match fs::canonicalize(&root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("unable to resolve root {}: {error}", root.display());
            return ExitCode::from(1);
        }
    };
    let report = receipt(&root, validate(&root));
    if let Some(path) = receipt_path.as_ref() {
        if let Err(error) = write_receipt(path, &report) {
            eprintln!("unable to write receipt {}: {error}", path.display());
            return ExitCode::from(1);
        }
    }
    if report.zero_unexplained_findings {
        println!(
            ".zpkg.toml Rust contract passed: repository + six language targets + installed closure + peer-authority gates"
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("STOPPED_FOR_EVALUATION: invalid Zed repository contract");
        for item in &report.findings {
            eprintln!("- {:?} {}: {}", item.code, item.path, item.detail);
        }
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("contract-parity lives below tools/")
            .to_path_buf()
    }

    fn copy_file(source_root: &Path, target_root: &Path, relative: &str) {
        let target = target_root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::copy(source_root.join(relative), target).expect("copy fixture source");
    }

    fn fixture() -> TempDir {
        let source_root = repository_root();
        let temporary = tempfile::tempdir().expect("fixture root");
        for relative in [
            ".zpkg.toml",
            "package.json",
            "contracts/authority-topology.json",
            "scripts/installed_package_smoke.py",
        ] {
            copy_file(&source_root, temporary.path(), relative);
        }
        for &(_, directory, _, _) in EXPECTED_TARGETS {
            if directory != "." {
                fs::create_dir_all(temporary.path().join(directory))
                    .expect("create target directory");
            }
        }
        for relative in REQUIRED_PATHS {
            let target = temporary.path().join(relative);
            if target.exists() {
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("create required file parent");
            }
            fs::write(target, b"fixture\n").expect("write required fixture file");
        }
        assert!(validate(temporary.path()).is_empty());
        temporary
    }

    fn has_code(findings: &[Finding], code: FindingCode) -> bool {
        findings.iter().any(|item| item.code == code)
    }

    #[test]
    fn repository_contract_passes() {
        let findings = validate(&repository_root());
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn package_language_is_rejected() {
        let fixture = fixture();
        let path = fixture.path().join(".zpkg.toml");
        let source = fs::read_to_string(&path).expect("read manifest");
        fs::write(
            &path,
            source.replacen(
                "name = \"ores-middleware\"\n",
                "name = \"ores-middleware\"\nlanguage = \"rust\"\n",
                1,
            ),
        )
        .expect("write drifted manifest");
        assert!(has_code(&validate(fixture.path()), FindingCode::PackageLanguage));
    }

    #[test]
    fn target_adapter_drift_is_rejected() {
        let fixture = fixture();
        let path = fixture.path().join(".zpkg.toml");
        let source = fs::read_to_string(&path).expect("read manifest");
        let source = source.replace(
            "[targets.golang]\ndir = \"src/golang\"\nname = \"ores-middleware-golang\"\nadapter = \"go\"",
            "[targets.golang]\ndir = \"src/golang\"\nname = \"ores-middleware-golang\"\nadapter = \"node\"",
        );
        fs::write(path, source).expect("write drifted target");
        assert!(has_code(&validate(fixture.path()), FindingCode::TargetField));
    }

    #[test]
    fn missing_control_file_is_rejected() {
        let fixture = fixture();
        fs::remove_file(fixture.path().join("scripts/orm_catalog_gate.py"))
            .expect("remove required fixture");
        assert!(has_code(&validate(fixture.path()), FindingCode::RequiredPath));
    }

    #[test]
    fn authority_fallback_is_rejected() {
        let fixture = fixture();
        let path = fixture.path().join("contracts/authority-topology.json");
        let mut topology: Value =
            serde_json::from_slice(&fs::read(&path).expect("read topology"))
                .expect("parse topology");
        topology["onUnexplainedMismatch"] = Value::String("PREFER_TYPESPEC".to_owned());
        fs::write(
            path,
            serde_json::to_vec_pretty(&topology).expect("serialize topology"),
        )
        .expect("write drifted topology");
        assert!(has_code(
            &validate(fixture.path()),
            FindingCode::AuthorityTopology
        ));
    }

    #[test]
    fn receipt_is_deterministic() {
        let fixture = fixture();
        let first = receipt(fixture.path(), validate(fixture.path()));
        let second = receipt(fixture.path(), validate(fixture.path()));
        assert_eq!(first, second);
        assert_eq!(first.status, "passed");
        assert!(first.zero_unexplained_findings);
        assert_eq!(first.source_digests.len(), DIGESTED_SOURCES.len());
    }
}
