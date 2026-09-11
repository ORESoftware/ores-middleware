#![forbid(unsafe_code)]

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const TYPESPEC_SOURCE: &str = "contracts/persistence/idempotency-record.tsp";
const JSON_SCHEMA_SOURCE: &str = "contracts/persistence/idempotency-record.schema.json";
const MODEL_NAME: &str = "IdempotencyRecord";

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StringConstraints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_length: Option<u64>,
}

impl StringConstraints {
    fn is_empty(&self) -> bool {
        self.pattern.is_none() && self.min_length.is_none() && self.max_length.is_none()
    }

    fn validate(&self, label: &str) -> Result<()> {
        if let (Some(minimum), Some(maximum)) = (self.min_length, self.max_length) {
            if minimum > maximum {
                return Err(format!(
                    "{label} has minimum length {minimum} greater than maximum length {maximum}"
                )
                .into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectClosure {
    additional_properties: bool,
    unevaluated_properties: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConstraintSnapshot {
    object_closure: ObjectClosure,
    fields: BTreeMap<String, StringConstraints>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Discrepancy {
    fingerprint: String,
    kind: String,
    detail: String,
    owner: String,
    resolution_state: String,
}

impl Discrepancy {
    fn new(kind: impl Into<String>, detail: impl Into<String>) -> Self {
        let kind = kind.into();
        let detail = detail.into();
        let mut digest = Sha256::new();
        digest.update(kind.as_bytes());
        digest.update([0]);
        digest.update(detail.as_bytes());
        Self {
            fingerprint: format!("{:x}", digest.finalize()),
            kind,
            detail,
            owner: "ORESoftware/ores-middleware".to_owned(),
            resolution_state: "unexplained".to_owned(),
        }
    }
}

fn extract_typespec_block(source: &str, keyword: &str, name: &str) -> Result<String> {
    let expression = format!(
        r"(?ms)\b{}\s+{}\s*\{{(?P<body>.*?)^\s*\}}",
        regex::escape(keyword),
        regex::escape(name)
    );
    let regex = Regex::new(&expression)?;
    regex
        .captures(source)
        .and_then(|captures| captures.name("body"))
        .map(|body| body.as_str().to_owned())
        .ok_or_else(|| format!("missing TypeSpec {keyword} {name}").into())
}

fn parse_bool_extension(source: &str, key: &str) -> Result<bool> {
    let expression = format!(
        r#"@TypeSpec\.JsonSchema\.extension\(\s*"{}"\s*,\s*(true|false)\s*\)"#,
        regex::escape(key)
    );
    let regex = Regex::new(&expression)?;
    let values = regex
        .captures_iter(source)
        .filter_map(|captures| captures.get(1))
        .map(|value| value.as_str() == "true")
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Ok(*value),
        [] => Err(format!(
            "TypeSpec {MODEL_NAME} must explicitly declare {key} with @TypeSpec.JsonSchema.extension"
        )
        .into()),
        _ => Err(format!(
            "TypeSpec {MODEL_NAME} declares {key} more than once"
        )
        .into()),
    }
}

fn parse_length_decorator(line: &str, decorator: &str) -> Result<Option<u64>> {
    let expression = format!(r"^@{}\(\s*([0-9]+)\s*\)$", regex::escape(decorator));
    let regex = Regex::new(&expression)?;
    regex
        .captures(line)
        .map(|captures| {
            captures[1]
                .parse::<u64>()
                .map_err(|error| format!("invalid @{decorator} value in {line}: {error}").into())
        })
        .transpose()
}

fn parse_pattern_decorator(line: &str) -> Result<Option<String>> {
    let regex = Regex::new(r#"^@pattern\((?P<literal>"(?:\\.|[^"\\])*")\)$"#)?;
    regex
        .captures(line)
        .and_then(|captures| captures.name("literal"))
        .map(|literal| {
            serde_json::from_str::<String>(literal.as_str()).map_err(|error| {
                format!("invalid @pattern string literal in {line}: {error}").into()
            })
        })
        .transpose()
}

fn parse_typespec(root: &Path) -> Result<ConstraintSnapshot> {
    let source = fs::read_to_string(root.join(TYPESPEC_SOURCE))?;
    let object_closure = ObjectClosure {
        additional_properties: parse_bool_extension(&source, "additionalProperties")?,
        unevaluated_properties: parse_bool_extension(&source, "unevaluatedProperties")?,
    };

    let property = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);$")?;
    let mut fields = BTreeMap::new();
    let mut pending = StringConstraints::default();

    for raw in extract_typespec_block(&source, "model", MODEL_NAME)?.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(value) = parse_length_decorator(line, "minLength")? {
            if pending.min_length.replace(value).is_some() {
                return Err("duplicate @minLength decorator".into());
            }
            continue;
        }
        if let Some(value) = parse_length_decorator(line, "maxLength")? {
            if pending.max_length.replace(value).is_some() {
                return Err("duplicate @maxLength decorator".into());
            }
            continue;
        }
        if let Some(value) = parse_pattern_decorator(line)? {
            if pending.pattern.replace(value).is_some() {
                return Err("duplicate @pattern decorator".into());
            }
            continue;
        }
        if line.starts_with('@') {
            return Err(format!(
                "unsupported TypeSpec field decorator in {MODEL_NAME}: {line}; add it to the Rust parity model before admission"
            )
            .into());
        }

        let captures = property
            .captures(line)
            .ok_or_else(|| format!("unsupported TypeSpec persistence property: {line}"))?;
        let name = captures[1].to_owned();
        let raw_type = captures[3].trim();
        if !pending.is_empty() && raw_type != "string" {
            return Err(format!(
                "string validation decorators on non-string TypeSpec field {name}: {raw_type}"
            )
            .into());
        }
        pending.validate(&format!("TypeSpec field {name}"))?;
        if fields.insert(name.clone(), pending.clone()).is_some() {
            return Err(format!("duplicate TypeSpec field {name}").into());
        }
        pending = StringConstraints::default();
    }

    if !pending.is_empty() {
        return Err("TypeSpec validation decorator is not attached to a field".into());
    }
    if fields.is_empty() {
        return Err(format!("TypeSpec {MODEL_NAME} must contain fields").into());
    }

    Ok(ConstraintSnapshot {
        object_closure,
        fields,
    })
}

fn json_bool(object: &Map<String, Value>, key: &str, label: &str) -> Result<bool> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{label}.{key} must be an explicit boolean").into())
}

fn reject_unsupported_json_keywords(object: &Map<String, Value>, label: &str) -> Result<()> {
    const UNSUPPORTED: &[&str] = &[
        "allOf",
        "anyOf",
        "const",
        "contains",
        "contentEncoding",
        "contentMediaType",
        "contentSchema",
        "dependentRequired",
        "dependentSchemas",
        "else",
        "exclusiveMaximum",
        "exclusiveMinimum",
        "if",
        "maxContains",
        "maxItems",
        "maxProperties",
        "minContains",
        "minItems",
        "minProperties",
        "multipleOf",
        "not",
        "oneOf",
        "patternProperties",
        "prefixItems",
        "propertyNames",
        "then",
        "uniqueItems",
    ];
    let found = UNSUPPORTED
        .iter()
        .filter(|key| object.contains_key(**key))
        .copied()
        .collect::<Vec<_>>();
    if found.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{label} uses unsupported semantic keyword(s) {}; extend the Rust parity model before admission",
            found.join(", ")
        )
        .into())
    }
}

fn json_u64(object: &Map<String, Value>, key: &str, label: &str) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| format!("{label}.{key} must be a non-negative integer").into())
        })
        .transpose()
}

fn parse_json_schema(root: &Path) -> Result<ConstraintSnapshot> {
    let document: Value = serde_json::from_slice(&fs::read(root.join(JSON_SCHEMA_SOURCE))?)?;
    let definitions = document
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or("JSON Schema must contain $defs")?;
    let model = definitions
        .get(MODEL_NAME)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("JSON Schema must contain $defs.{MODEL_NAME}"))?;
    reject_unsupported_json_keywords(model, &format!("$defs.{MODEL_NAME}"))?;

    let object_closure = ObjectClosure {
        additional_properties: json_bool(model, "additionalProperties", MODEL_NAME)?,
        unevaluated_properties: json_bool(model, "unevaluatedProperties", MODEL_NAME)?,
    };
    let properties = model
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("$defs.{MODEL_NAME}.properties must be an object"))?;
    let mut fields = BTreeMap::new();

    for (name, value) in properties {
        let property = value
            .as_object()
            .ok_or_else(|| format!("JSON Schema property {name} must be an object"))?;
        reject_unsupported_json_keywords(
            property,
            &format!("$defs.{MODEL_NAME}.properties.{name}"),
        )?;
        let constraints = StringConstraints {
            pattern: property
                .get("pattern")
                .map(|value| {
                    value.as_str().map(str::to_owned).ok_or_else(|| {
                        format!("JSON Schema property {name}.pattern must be a string")
                    })
                })
                .transpose()?,
            min_length: json_u64(property, "minLength", &format!("property {name}"))?,
            max_length: json_u64(property, "maxLength", &format!("property {name}"))?,
        };
        if !constraints.is_empty() && property.get("type").and_then(Value::as_str) != Some("string")
        {
            return Err(format!(
                "string constraints on non-string JSON Schema property {name}: {value}"
            )
            .into());
        }
        constraints.validate(&format!("JSON Schema property {name}"))?;
        fields.insert(name.clone(), constraints);
    }
    if fields.is_empty() {
        return Err(format!("JSON Schema {MODEL_NAME} must contain fields").into());
    }

    Ok(ConstraintSnapshot {
        object_closure,
        fields,
    })
}

fn compare_snapshots(
    typespec: &ConstraintSnapshot,
    json_schema: &ConstraintSnapshot,
) -> Vec<Discrepancy> {
    let mut findings = Vec::new();
    if typespec.object_closure != json_schema.object_closure {
        findings.push(Discrepancy::new(
            "object-closure-mismatch",
            format!(
                "TypeSpec={} JSON-Schema={}",
                serde_json::to_string(&typespec.object_closure).expect("closure serializes"),
                serde_json::to_string(&json_schema.object_closure).expect("closure serializes")
            ),
        ));
    }

    let field_names = typespec
        .fields
        .keys()
        .chain(json_schema.fields.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in field_names {
        match (typespec.fields.get(&name), json_schema.fields.get(&name)) {
            (Some(left), Some(right)) if left == right => {}
            (Some(left), Some(right)) => findings.push(Discrepancy::new(
                "field-constraint-mismatch",
                format!(
                    "field {name}: TypeSpec={} JSON-Schema={}",
                    serde_json::to_string(left).expect("constraint serializes"),
                    serde_json::to_string(right).expect("constraint serializes")
                ),
            )),
            (Some(_), None) => findings.push(Discrepancy::new(
                "field-constraint-surface-mismatch",
                format!("field {name} exists only in TypeSpec constraint surface"),
            )),
            (None, Some(_)) => findings.push(Discrepancy::new(
                "field-constraint-surface-mismatch",
                format!("field {name} exists only in JSON Schema constraint surface"),
            )),
            (None, None) => unreachable!("field name came from at least one map"),
        }
    }
    findings
}

fn run(root: &Path) -> Result<(ConstraintSnapshot, ConstraintSnapshot, Vec<Discrepancy>)> {
    let typespec = parse_typespec(root)?;
    let json_schema = parse_json_schema(root)?;
    let findings = compare_snapshots(&typespec, &json_schema);
    Ok((typespec, json_schema, findings))
}

fn write_report(
    path: &Path,
    typespec: Option<&ConstraintSnapshot>,
    json_schema: Option<&ConstraintSnapshot>,
    findings: &[Discrepancy],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = json!({
        "schema": "ores.persistence-constraint-parity/v1",
        "authorities": ["typespec", "json-schema-openapi"],
        "sources": {
            "typespec": TYPESPEC_SOURCE,
            "jsonSchemaOpenapi": JSON_SCHEMA_SOURCE,
        },
        "scope": [
            "object closure",
            "field pattern",
            "field minimum length",
            "field maximum length",
            "unsupported semantic keyword rejection",
        ],
        "snapshots": {
            "typespec": typespec,
            "jsonSchemaOpenapi": json_schema,
        },
        "status": if findings.is_empty() { "passed" } else { "stopped_for_evaluation" },
        "zeroUnexplainedFindings": findings.is_empty(),
        "discrepancies": findings,
    });
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    Ok(())
}

fn resolve(root: &Path, value: PathBuf) -> Result<PathBuf> {
    let relative = if value.is_absolute() {
        value
            .strip_prefix(root)
            .map_err(|_| format!("report path must remain inside {}", root.display()))?
    } else {
        value.as_path()
    };
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "report path must be a normalized descendant of {}",
            root.display()
        )
        .into());
    }
    Ok(root.join(relative))
}

fn parse_args() -> std::result::Result<(PathBuf, PathBuf), String> {
    let mut root = env::current_dir().map_err(|error| error.to_string())?;
    let mut report = PathBuf::from("target/schema-convergence/constraint-parity.json");
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
                report = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--report requires a path".to_owned())?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "Usage: persistence_constraint_gate [--root PATH] [--report PATH]\n\
                     Compare object closure and field constraints across independent TypeSpec and JSON Schema authorities."
                );
                return Err(String::new());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    root = root
        .canonicalize()
        .map_err(|error| format!("invalid root {}: {error}", root.display()))?;
    let report = resolve(&root, report).map_err(|error| error.to_string())?;
    Ok((root, report))
}

fn main() -> ExitCode {
    let (root, report) = match parse_args() {
        Ok(arguments) => arguments,
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("persistence constraint parity: {message}");
            return ExitCode::from(64);
        }
    };

    match run(&root) {
        Ok((typespec, json_schema, findings)) => {
            if let Err(error) =
                write_report(&report, Some(&typespec), Some(&json_schema), &findings)
            {
                eprintln!("failed to write {}: {error}", report.display());
                return ExitCode::from(2);
            }
            if findings.is_empty() {
                println!(
                    "persistence object closure and field constraint parity passed; report={}",
                    report.display()
                );
                ExitCode::SUCCESS
            } else {
                println!(
                    "STOPPED_FOR_EVALUATION: {} persistence constraint discrepancy(s); report={}",
                    findings.len(),
                    report.display()
                );
                for finding in findings {
                    println!(
                        "- {}: {}: {}",
                        finding.fingerprint, finding.kind, finding.detail
                    );
                }
                ExitCode::from(2)
            }
        }
        Err(error) => {
            let finding = Discrepancy::new("constraint-check-failure", error.to_string());
            let _ = write_report(&report, None, None, std::slice::from_ref(&finding));
            eprintln!(
                "STOPPED_FOR_EVALUATION: {}: {}; report={}",
                finding.kind,
                finding.detail,
                report.display()
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn fixture_root() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        let persistence = temp.path().join("contracts/persistence");
        fs::create_dir_all(&persistence).expect("create persistence folder");
        for relative in [TYPESPEC_SOURCE, JSON_SCHEMA_SOURCE] {
            fs::copy(repository_root().join(relative), temp.path().join(relative))
                .expect("copy authority");
        }
        temp
    }

    fn mutate_json(root: &Path, edit: impl FnOnce(&mut Value)) {
        let path = root.join(JSON_SCHEMA_SOURCE);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut document);
        fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    #[test]
    fn current_authorities_have_equal_constraints_and_closed_objects() {
        let temp = fixture_root();
        let (typespec, json_schema, findings) = run(temp.path()).expect("gate runs");
        assert_eq!(findings, Vec::new());
        assert_eq!(typespec, json_schema);
        assert!(!typespec.object_closure.additional_properties);
        assert!(!typespec.object_closure.unevaluated_properties);
    }

    #[test]
    fn json_pattern_without_typespec_peer_stops_evaluation() {
        let temp = fixture_root();
        mutate_json(temp.path(), |document| {
            document["$defs"][MODEL_NAME]["properties"]["requestHash"]["pattern"] =
                json!("^[0-9a-f]{64}$");
        });
        let (_, _, findings) = run(temp.path()).expect("gate runs");
        assert!(findings.iter().any(|finding| {
            finding.kind == "field-constraint-mismatch" && finding.detail.contains("requestHash")
        }));
    }

    #[test]
    fn typespec_length_without_json_peer_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join(TYPESPEC_SOURCE);
        let source = fs::read_to_string(&path).unwrap().replace(
            "  requestHash: string;",
            "  @minLength(1)\n  requestHash: string;",
        );
        fs::write(&path, source).unwrap();
        let (_, _, findings) = run(temp.path()).expect("gate runs");
        assert!(findings.iter().any(|finding| {
            finding.kind == "field-constraint-mismatch" && finding.detail.contains("requestHash")
        }));
    }

    #[test]
    fn aligned_string_constraints_pass() {
        let temp = fixture_root();
        let path = temp.path().join(TYPESPEC_SOURCE);
        let source = fs::read_to_string(&path).unwrap().replace(
            "  requestHash: string;",
            "  @minLength(1)\n  @maxLength(128)\n  @pattern(\"^[A-Za-z0-9_-]+$\")\n  requestHash: string;",
        );
        fs::write(&path, source).unwrap();
        mutate_json(temp.path(), |document| {
            let property = &mut document["$defs"][MODEL_NAME]["properties"]["requestHash"];
            property["minLength"] = json!(1);
            property["maxLength"] = json!(128);
            property["pattern"] = json!("^[A-Za-z0-9_-]+$");
        });
        let (_, _, findings) = run(temp.path()).expect("gate runs");
        assert_eq!(findings, Vec::new());
    }

    #[test]
    fn reopened_json_object_stops_evaluation() {
        let temp = fixture_root();
        mutate_json(temp.path(), |document| {
            document["$defs"][MODEL_NAME]["additionalProperties"] = json!(true);
        });
        let (_, _, findings) = run(temp.path()).expect("gate runs");
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == "object-closure-mismatch")
        );
    }

    #[test]
    fn missing_typespec_closure_is_a_hard_failure() {
        let temp = fixture_root();
        let path = temp.path().join(TYPESPEC_SOURCE);
        let source = fs::read_to_string(&path).unwrap().replace(
            "@TypeSpec.JsonSchema.extension(\"additionalProperties\", false)\n",
            "",
        );
        fs::write(&path, source).unwrap();
        let error = run(temp.path()).expect_err("missing closure must fail");
        assert!(error.to_string().contains("additionalProperties"));
    }

    #[test]
    fn unsupported_json_semantics_fail_closed() {
        let temp = fixture_root();
        mutate_json(temp.path(), |document| {
            document["$defs"][MODEL_NAME]["patternProperties"] = json!({"^x-": true});
        });
        let error = run(temp.path()).expect_err("unsupported keyword must fail");
        assert!(error.to_string().contains("patternProperties"));
    }

    #[test]
    fn unsupported_typespec_field_decorator_fails_closed() {
        let temp = fixture_root();
        let path = temp.path().join(TYPESPEC_SOURCE);
        let source = fs::read_to_string(&path).unwrap().replace(
            "  requestHash: string;",
            "  @secret\n  requestHash: string;",
        );
        fs::write(&path, source).unwrap();
        let error = run(temp.path()).expect_err("unsupported decorator must fail");
        assert!(error.to_string().contains("@secret"));
    }

    #[test]
    fn report_path_traversal_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        assert!(resolve(root.path(), PathBuf::from("../outside.json")).is_err());
        assert!(resolve(root.path(), root.path().join("../outside.json")).is_err());
        assert_eq!(
            resolve(root.path(), PathBuf::from("target/report.json")).unwrap(),
            root.path().join("target/report.json")
        );
    }

    #[test]
    fn impossible_length_range_is_rejected() {
        let temp = fixture_root();
        mutate_json(temp.path(), |document| {
            let property = &mut document["$defs"][MODEL_NAME]["properties"]["requestHash"];
            property["minLength"] = json!(10);
            property["maxLength"] = json!(2);
        });
        let error = run(temp.path()).expect_err("invalid range must fail");
        assert!(error.to_string().contains("greater than"));
    }
}
